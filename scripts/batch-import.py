#!/usr/bin/env python3
"""Batch import documents into CogKOS.

Usage:
    python scripts/batch-import.py /path/to/docs/ --url http://localhost:3000/mcp --api-key your-key

Supports: PDF, DOCX, XLSX, CSV, MD, TXT, JSON, XML, YAML, HTML, PNG, JPG
Skips: files > 50MB, unsupported formats
"""
from __future__ import annotations
import argparse, base64, hashlib, json, os, sys, time, urllib.error, urllib.request
from pathlib import Path

SUPPORTED = {".pdf",".docx",".xlsx",".csv",".md",".txt",".json",".xml",".yaml",".yml",".html",".htm",".png",".jpg",".jpeg"}
MAX_SIZE = 50 * 1024 * 1024
_JSONRPC = "2.0"
_MCP_VERSION = "2025-03-26"


class MCPClient:
    """Minimal MCP client using stdlib urllib."""
    def __init__(self, url: str, api_key: str, tenant_id: str, timeout: float = 300):
        self._url, self._api_key, self._tenant_id = url.rstrip("/"), api_key, tenant_id
        self._timeout, self._sid, self._rid = timeout, None, 0

    def _post(self, payload: dict) -> dict:
        headers = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream",
                    "X-API-Key": self._api_key, "X-Tenant-ID": self._tenant_id}
        if self._sid: headers["mcp-session-id"] = self._sid
        req = urllib.request.Request(self._url, json.dumps(payload).encode(), headers)
        resp = urllib.request.urlopen(req, timeout=self._timeout)
        if sid := resp.headers.get("mcp-session-id"): self._sid = sid
        body = resp.read().decode()
        ct = resp.headers.get("content-type", "")
        if "text/event-stream" in ct:
            result = {}
            for line in body.splitlines():
                if line.strip().startswith("data:"):
                    try: result = json.loads(line.strip()[5:].strip())
                    except json.JSONDecodeError: pass
            return result
        return json.loads(body)

    def _nid(self) -> int:
        self._rid += 1; return self._rid

    def init_session(self) -> None:
        r = self._post({"jsonrpc": _JSONRPC, "id": self._nid(), "method": "initialize",
            "params": {"protocolVersion": _MCP_VERSION, "capabilities": {},
                        "clientInfo": {"name": "batch-import", "version": "0.1.0"}}})
        if "error" in r: raise RuntimeError(f"MCP init failed: {r['error']}")
        self._post({"jsonrpc": _JSONRPC, "method": "notifications/initialized"})

    def upload(self, filename: str, content_b64: str, tags: list[str]) -> dict:
        if not self._sid: self.init_session()
        r = self._post({"jsonrpc": _JSONRPC, "id": self._nid(), "method": "tools/call",
            "params": {"name": "upload_document", "arguments": {
                "filename": filename, "content_base64": content_b64,
                "source": {"type": "human", "user_id": "batch-import", "role": "user"},
                "tags": tags, "auto_process": True}}})
        if "error" in r: raise RuntimeError(r["error"].get("message", str(r["error"])))
        return r


def file_hash(p: Path) -> str:
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(8192), b""): h.update(chunk)
    return h.hexdigest()

def fmt_size(s: int) -> str:
    if s < 1024: return f"{s}B"
    return f"{s/1024:.1f}KB" if s < 1048576 else f"{s/1048576:.1f}MB"

def scan(d: Path) -> list[Path]:
    files = [p for p in d.rglob("*") if p.is_file() and p.suffix.lower() in SUPPORTED and p.stat().st_size <= MAX_SIZE]
    files.sort(key=lambda p: p.stat().st_size)
    return files

def main() -> int:
    ap = argparse.ArgumentParser(description="Batch import documents into CogKOS",
        epilog="Supported: PDF,DOCX,XLSX,CSV,MD,TXT,JSON,XML,YAML,HTML,PNG,JPG. Max 50MB per file.\n"
               "Env: COGKOS_URL, COGKOS_API_KEY, COGKOS_TENANT_ID",
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("directory", type=Path, help="Directory to scan")
    ap.add_argument("--url", default=os.environ.get("COGKOS_URL", "http://localhost:3000/mcp"))
    ap.add_argument("--api-key", default=os.environ.get("COGKOS_API_KEY", ""))
    ap.add_argument("--tenant-id", default=os.environ.get("COGKOS_TENANT_ID", "default"))
    ap.add_argument("--tags", nargs="*", default=[], help="Tags for all imports")
    ap.add_argument("--dry-run", action="store_true", help="List files without importing")
    ap.add_argument("--hash-cache", type=Path, default=None, help="Hash cache file for dedup")
    args = ap.parse_args()

    if not args.directory.is_dir():
        print(f"ERROR: {args.directory} is not a directory", file=sys.stderr); return 1
    if not args.api_key and not args.dry_run:
        print("ERROR: --api-key or COGKOS_API_KEY required", file=sys.stderr); return 1

    files = scan(args.directory)
    if not files: print("No supported files found."); return 0
    print(f"Found {len(files)} files in {args.directory}")
    if args.dry_run:
        for f in files: print(f"  {f.name} ({fmt_size(f.stat().st_size)})")
        return 0

    hc_path = args.hash_cache or (args.directory / ".cogkos-import-hashes")
    known = set(hc_path.read_text().strip().splitlines()) if hc_path.exists() else set()
    client = MCPClient(args.url, args.api_key, args.tenant_id)
    ok = fail = dup = 0

    for i, fp in enumerate(files, 1):
        fh, sz = file_hash(fp), fmt_size(fp.stat().st_size)
        if fh in known:
            print(f"[{i}/{len(files)}] = {fp.name} ({sz}) duplicate"); dup += 1; continue
        t0 = time.monotonic()
        try:
            client.upload(fp.name, base64.b64encode(fp.read_bytes()).decode("ascii"), args.tags)
            print(f"[{i}/{len(files)}] ok {fp.name} ({sz}, {time.monotonic()-t0:.1f}s)")
            known.add(fh); ok += 1
        except Exception as e:
            print(f"[{i}/{len(files)}] FAIL {fp.name} ({sz}, {time.monotonic()-t0:.1f}s): {e}", file=sys.stderr)
            fail += 1

    hc_path.write_text("\n".join(sorted(known)) + "\n")
    print(f"\nSummary: {ok} success, {fail} failed, {dup} duplicate")
    return 1 if fail else 0

if __name__ == "__main__":
    sys.exit(main())
