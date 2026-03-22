#!/usr/bin/env python3
"""Re-embed all claims using the current embedding model.

Use after switching embedding models (e.g., text-embedding-3-large -> BGE-M3).

Usage:
    python scripts/reembed.py --db-url postgres://cogkos:cogkos_dev@localhost:5435/cogkos \
                              --embedding-url http://localhost:8090/v1 \
                              --batch-size 32

Environment variables (fallbacks for CLI args):
    DATABASE_URL        PostgreSQL connection string
    EMBEDDING_BASE_URL  Embedding API base URL
    EMBEDDING_API_KEY   API key for embedding service (optional for local TEI)
    EMBEDDING_MODEL     Model name (default: BAAI/bge-m3)
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.request
import urllib.error

# Optional psycopg2 import — fall back to helpful error message
try:
    import psycopg2
    import psycopg2.extras
except ImportError:
    print("ERROR: psycopg2 is required. Install with: pip install psycopg2-binary", file=sys.stderr)
    sys.exit(1)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Re-embed all claims using the current embedding model."
    )
    parser.add_argument(
        "--db-url",
        default=os.environ.get("DATABASE_URL", ""),
        help="PostgreSQL connection string (or set DATABASE_URL env var)",
    )
    parser.add_argument(
        "--embedding-url",
        default=os.environ.get("EMBEDDING_BASE_URL", "http://localhost:8090/v1"),
        help="Embedding API base URL (OpenAI-compatible)",
    )
    parser.add_argument(
        "--embedding-key",
        default=os.environ.get("EMBEDDING_API_KEY", os.environ.get("API_302_KEY", "")),
        help="API key for embedding service (optional for local TEI)",
    )
    parser.add_argument(
        "--model",
        default=os.environ.get("EMBEDDING_MODEL", "BAAI/bge-m3"),
        help="Embedding model name",
    )
    parser.add_argument(
        "--batch-size",
        type=int,
        default=32,
        help="Number of claims per embedding batch (default: 32)",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Re-embed ALL claims, not just those with NULL embeddings",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Count claims to process without actually embedding",
    )
    return parser.parse_args()


def get_embeddings(
    texts: list[str],
    base_url: str,
    api_key: str,
    model: str,
) -> list[list[float]]:
    """Call OpenAI-compatible embeddings endpoint."""
    url = f"{base_url.rstrip('/')}/embeddings"
    payload = json.dumps({"input": texts, "model": model}).encode()

    headers = {"Content-Type": "application/json"}
    if api_key:
        headers["Authorization"] = f"Bearer {api_key}"

    req = urllib.request.Request(url, data=payload, headers=headers, method="POST")

    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            data = json.loads(resp.read())
    except urllib.error.HTTPError as e:
        body = e.read().decode(errors="replace")[:500]
        print(f"ERROR: Embedding API returned {e.code}: {body}", file=sys.stderr)
        raise SystemExit(1)
    except urllib.error.URLError as e:
        print(f"ERROR: Cannot reach embedding API at {url}: {e.reason}", file=sys.stderr)
        raise SystemExit(1)

    # Sort by index to preserve order
    sorted_data = sorted(data["data"], key=lambda x: x["index"])
    return [item["embedding"] for item in sorted_data]


def main() -> None:
    args = parse_args()

    if not args.db_url:
        print("ERROR: --db-url or DATABASE_URL env var is required", file=sys.stderr)
        sys.exit(1)

    conn = psycopg2.connect(args.db_url)
    conn.autocommit = False
    cur = conn.cursor(cursor_factory=psycopg2.extras.DictCursor)

    # Count total claims to process
    if args.force:
        cur.execute("SELECT COUNT(*) FROM epistemic_claims")
    else:
        cur.execute("SELECT COUNT(*) FROM epistemic_claims WHERE embedding IS NULL")
    total = cur.fetchone()[0]

    print(f"Claims to process: {total} ({'all' if args.force else 'missing embeddings only'})")
    print(f"Model: {args.model}")
    print(f"API: {args.embedding_url}")
    print(f"Batch size: {args.batch_size}")

    if args.dry_run:
        print("Dry run — exiting.")
        cur.close()
        conn.close()
        return

    if total == 0:
        print("Nothing to do.")
        cur.close()
        conn.close()
        return

    # Process in batches
    processed = 0
    offset = 0
    t_start = time.monotonic()

    while offset < total:
        if args.force:
            cur.execute(
                "SELECT id, content FROM epistemic_claims ORDER BY id LIMIT %s OFFSET %s",
                (args.batch_size, offset),
            )
        else:
            cur.execute(
                "SELECT id, content FROM epistemic_claims WHERE embedding IS NULL ORDER BY id LIMIT %s OFFSET %s",
                (args.batch_size, offset),
            )

        rows = cur.fetchall()
        if not rows:
            break

        ids = [r["id"] for r in rows]
        contents = [r["content"] for r in rows]

        # Get embeddings
        vectors = get_embeddings(contents, args.embedding_url, args.embedding_key, args.model)

        # Write back
        update_cur = conn.cursor()
        for claim_id, vector in zip(ids, vectors):
            vector_str = "[" + ",".join(str(v) for v in vector) + "]"
            update_cur.execute(
                "UPDATE epistemic_claims SET embedding = %s::vector, updated_at = NOW() WHERE id = %s",
                (vector_str, claim_id),
            )
        conn.commit()
        update_cur.close()

        processed += len(rows)
        elapsed = time.monotonic() - t_start
        rate = processed / elapsed if elapsed > 0 else 0
        print(f"  [{processed}/{total}] {rate:.1f} claims/s", flush=True)

        offset += args.batch_size

    elapsed = time.monotonic() - t_start
    print(f"Done. Processed {processed} claims in {elapsed:.1f}s")

    cur.close()
    conn.close()


if __name__ == "__main__":
    main()
