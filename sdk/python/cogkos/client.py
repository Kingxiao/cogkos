"""CogKOS Python SDK — MCP Streamable HTTP client.

Thin wrapper over CogKOS MCP server. No async, no MCP SDK dependency.
Uses httpx for HTTP + SSE parsing.
"""

from __future__ import annotations

import json
import logging
from typing import Any

import httpx

from .models import (
    FeedbackResult,
    GapResult,
    LearnResult,
    RecallResult,
)

logger = logging.getLogger("cogkos")

_JSONRPC_VERSION = "2.0"
_MCP_PROTOCOL_VERSION = "2025-03-26"
_CLIENT_NAME = "cogkos-python-sdk"
_CLIENT_VERSION = "0.1.0"
_SSE_HEADERS = {
    "Accept": "application/json, text/event-stream",
    "Content-Type": "application/json",
}


class CogKOSError(Exception):
    """Base exception for CogKOS SDK errors."""

    def __init__(self, message: str, code: int | None = None, data: Any = None):
        super().__init__(message)
        self.code = code
        self.data = data


class SessionExpiredError(CogKOSError):
    """Raised when the MCP session has expired and needs re-init."""


class CogKOS:
    """Synchronous CogKOS client over MCP Streamable HTTP or native REST.

    Usage (MCP mode, default)::

        brain = CogKOS("http://localhost:3000/mcp", api_key="xxx", tenant_id="dev")
        result = brain.recall("multi-tenant isolation")
        print(result.best_belief)

    Usage (REST mode — lower latency, bypasses MCP protocol layer)::

        brain = CogKOS("http://localhost:3000", api_key="xxx", tenant_id="dev", use_rest=True)
        result = brain.recall("multi-tenant isolation")
    """

    def __init__(
        self,
        url: str,
        api_key: str,
        tenant_id: str,
        *,
        timeout: float = 300.0,
        source_agent: str = "python-sdk",
        use_rest: bool = False,
    ):
        self._url = url.rstrip("/")
        self._api_key = api_key
        self._tenant_id = tenant_id
        self._timeout = timeout
        self._source_agent = source_agent
        self._use_rest = use_rest

        self._session_id: str | None = None
        self._request_id = 0
        self._http = httpx.Client(timeout=timeout)

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def learn(
        self,
        content: str,
        *,
        confidence: float = 0.8,
        node_type: str = "Insight",
        knowledge_type: str | None = None,
        tags: list[str] | None = None,
        source_agent: str | None = None,
        memory_layer: str | None = None,
        session_id: str | None = None,
        namespace: str | None = None,
    ) -> LearnResult:
        """Submit knowledge to CogKOS.

        Args:
            content: The knowledge content to store.
            confidence: Confidence level 0.0-1.0 (default 0.8).
            node_type: Type of knowledge node (Entity, Relation, Event, Attribute, Prediction, Insight, File).
            knowledge_type: Knowledge authority tier — "Business" or "Experiential".
            tags: Optional tags for categorization.
            source_agent: Override the default source agent name.
            memory_layer: Memory layer (working/episodic/semantic).
            session_id: Session ID for working/episodic scoping.
            namespace: Namespace for intra-tenant isolation (e.g. client project scoping).

        Returns:
            LearnResult with claim_id and status.
        """
        agent = source_agent or self._source_agent
        args: dict[str, Any] = {
            "content": content,
            "node_type": node_type,
            "confidence": confidence,
            "source": {"type": "agent", "agent_id": agent, "model": agent},
            "tags": tags or [],
        }
        if knowledge_type:
            args["knowledge_type"] = knowledge_type
        if memory_layer:
            args["memory_layer"] = memory_layer
        if session_id:
            args["session_id"] = session_id
        if namespace:
            args["namespace"] = namespace

        if self._use_rest:
            data = self._rest_post("/api/v1/learn", args)
        else:
            data = self._call_tool("submit_experience", args)
        return LearnResult.from_response(data)

    def recall(
        self,
        query: str,
        *,
        domain: str | None = None,
        max_results: int = 10,
        include_predictions: bool = True,
        include_conflicts: bool = True,
        include_gaps: bool = True,
        memory_layer: str | None = None,
        session_id: str | None = None,
        namespace: str | None = None,
    ) -> RecallResult:
        """Query the knowledge base.

        Args:
            query: Natural language query.
            domain: Optional domain filter.
            max_results: Maximum results to return.
            include_predictions: Include prediction data.
            include_conflicts: Include conflict info.
            include_gaps: Include knowledge gaps.
            memory_layer: Filter by memory layer.
            session_id: Filter by session ID.
            namespace: Namespace for intra-tenant isolation.

        Returns:
            RecallResult with beliefs, conflicts, predictions, etc.
        """
        args: dict[str, Any] = {
            "query": query,
            "context": {"max_results": max_results},
            "include_predictions": include_predictions,
            "include_conflicts": include_conflicts,
            "include_gaps": include_gaps,
        }
        if domain:
            args["context"]["domain"] = domain
        if memory_layer:
            args["memory_layer"] = memory_layer
        if session_id:
            args["session_id"] = session_id
        if namespace:
            args["namespace"] = namespace

        if self._use_rest:
            # REST API uses flat query params instead of nested MCP args
            rest_args: dict[str, Any] = {
                "query": query,
                "max_results": max_results,
                "include_predictions": include_predictions,
                "include_conflicts": include_conflicts,
                "include_gaps": include_gaps,
            }
            if domain:
                rest_args["domain"] = domain
            if memory_layer:
                rest_args["memory_layer"] = memory_layer
            if session_id:
                rest_args["session_id"] = session_id
            if namespace:
                rest_args["namespace"] = namespace
            data = self._rest_post("/api/v1/query", rest_args)
        else:
            data = self._call_tool("query_knowledge", args)
        return RecallResult.from_response(data)

    def feedback(
        self,
        query_hash: int,
        *,
        success: bool,
        note: str | None = None,
    ) -> FeedbackResult:
        """Submit feedback on a previous query result.

        Args:
            query_hash: The query_hash from a RecallResult.
            success: Whether the result was useful.
            note: Optional textual feedback.

        Returns:
            FeedbackResult with status.
        """
        args: dict[str, Any] = {
            "query_hash": query_hash,
            "success": success,
        }
        if note:
            args["note"] = note

        if self._use_rest:
            data = self._rest_post("/api/v1/feedback", args)
        else:
            data = self._call_tool("submit_feedback", args)
        return FeedbackResult.from_response(data)

    def report_gap(
        self,
        domain: str,
        description: str,
        *,
        priority: str = "medium",
    ) -> GapResult:
        """Report a knowledge gap.

        Args:
            domain: The knowledge domain.
            description: Description of what's missing.
            priority: Priority level (low/medium/high).

        Returns:
            GapResult with gap_id.
        """
        args: dict[str, Any] = {
            "domain": domain,
            "description": description,
            "priority": priority,
        }
        data = self._call_tool("report_gap", args)
        return GapResult.from_response(data)

    def close(self) -> None:
        """Close the HTTP client."""
        self._http.close()

    def __enter__(self) -> CogKOS:
        return self

    def __exit__(self, *_: Any) -> None:
        self.close()

    # ------------------------------------------------------------------
    # REST API internals (bypass MCP protocol for lower latency)
    # ------------------------------------------------------------------

    def _rest_headers(self) -> dict[str, str]:
        return {
            "Content-Type": "application/json",
            "X-API-Key": self._api_key,
            "X-Tenant-ID": self._tenant_id,
        }

    def _rest_post(self, path: str, payload: dict[str, Any]) -> dict[str, Any]:
        """Direct HTTP POST to REST API endpoint. No MCP framing."""
        url = self._url + path
        resp = self._http.post(url, json=payload, headers=self._rest_headers())
        if resp.status_code >= 400:
            try:
                body = resp.json()
                msg = body.get("error", resp.text[:500])
            except Exception:
                msg = resp.text[:500]
            raise CogKOSError(
                f"REST API error {resp.status_code}: {msg}",
                code=resp.status_code,
            )
        return resp.json()

    # ------------------------------------------------------------------
    # MCP protocol internals
    # ------------------------------------------------------------------

    def _next_id(self) -> int:
        self._request_id += 1
        return self._request_id

    def _headers(self) -> dict[str, str]:
        headers = {
            **_SSE_HEADERS,
            "X-API-Key": self._api_key,
            "X-Tenant-ID": self._tenant_id,
        }
        if self._session_id:
            headers["mcp-session-id"] = self._session_id
        return headers

    def _post(self, payload: dict[str, Any]) -> httpx.Response:
        resp = self._http.post(self._url, json=payload, headers=self._headers())
        # Capture session ID from response headers
        sid = resp.headers.get("mcp-session-id")
        if sid:
            self._session_id = sid
        return resp

    def _parse_sse(self, text: str) -> dict[str, Any] | None:
        """Parse SSE response text, extract last JSON-RPC result."""
        result = None
        for line in text.splitlines():
            stripped = line.strip()
            if stripped.startswith("data:"):
                data_str = stripped[5:].strip()
                if not data_str:
                    continue
                try:
                    parsed = json.loads(data_str)
                    result = parsed
                except json.JSONDecodeError:
                    continue
        return result

    def _parse_response(self, resp: httpx.Response) -> dict[str, Any]:
        """Parse MCP response (JSON or SSE)."""
        content_type = resp.headers.get("content-type", "")

        if resp.status_code == 404 or resp.status_code == 410:
            raise SessionExpiredError(
                f"Session expired (HTTP {resp.status_code})",
                code=resp.status_code,
            )

        if resp.status_code >= 400:
            raise CogKOSError(
                f"HTTP error {resp.status_code}: {resp.text[:500]}",
                code=resp.status_code,
            )

        # Direct JSON response
        if "application/json" in content_type:
            return resp.json()

        # SSE response
        if "text/event-stream" in content_type:
            parsed = self._parse_sse(resp.text)
            if parsed is None:
                raise CogKOSError(f"Empty SSE response: {resp.text[:500]}")
            return parsed

        # Fallback: try JSON
        try:
            return resp.json()
        except Exception:
            raise CogKOSError(f"Unexpected content-type: {content_type}")

    def _ensure_session(self) -> None:
        """Perform MCP initialize + initialized handshake."""
        logger.debug("Initializing MCP session at %s", self._url)

        # Step 1: initialize
        init_payload = {
            "jsonrpc": _JSONRPC_VERSION,
            "id": self._next_id(),
            "method": "initialize",
            "params": {
                "protocolVersion": _MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": _CLIENT_NAME,
                    "version": _CLIENT_VERSION,
                },
            },
        }
        resp = self._post(init_payload)
        result = self._parse_response(resp)

        if "error" in result:
            err = result["error"]
            raise CogKOSError(
                f"MCP initialize failed: {err.get('message', err)}",
                code=err.get("code"),
                data=err.get("data"),
            )

        logger.debug("MCP session initialized: %s", self._session_id)

        # Step 2: notifications/initialized
        notif_payload = {
            "jsonrpc": _JSONRPC_VERSION,
            "method": "notifications/initialized",
        }
        self._post(notif_payload)

    def _call_tool(self, name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        """Call an MCP tool, with auto-init and session recovery.

        Returns parsed tool result as dict.
        """
        return self._call_tool_inner(name, arguments, retry=True)

    def _call_tool_inner(
        self, name: str, arguments: dict[str, Any], *, retry: bool
    ) -> dict[str, Any]:
        # Lazy init
        if self._session_id is None:
            self._ensure_session()

        payload = {
            "jsonrpc": _JSONRPC_VERSION,
            "id": self._next_id(),
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
            },
        }

        try:
            resp = self._post(payload)
            result = self._parse_response(resp)
        except SessionExpiredError:
            if retry:
                logger.info("Session expired, re-initializing...")
                self._session_id = None
                return self._call_tool_inner(name, arguments, retry=False)
            raise

        # Check JSON-RPC error
        if "error" in result:
            err = result["error"]
            msg = err.get("message", str(err))
            raise CogKOSError(
                f"Tool '{name}' failed: {msg}",
                code=err.get("code"),
                data=err.get("data"),
            )

        # Extract tool result content
        tool_result = result.get("result", result)

        # MCP tool results have {content: [{type: "text", text: "..."}]}
        content_list = tool_result.get("content", [])
        if content_list and isinstance(content_list, list):
            for item in content_list:
                if item.get("type") == "text":
                    text = item["text"]
                    try:
                        return json.loads(text)
                    except json.JSONDecodeError:
                        return {"raw_text": text}

        # Fallback: return as-is
        if isinstance(tool_result, dict):
            return tool_result
        return {"raw": tool_result}
