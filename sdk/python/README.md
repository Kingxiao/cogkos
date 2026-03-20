# CogKOS Python SDK

Thin Python client for CogKOS MCP Streamable HTTP interface.

## Install

```bash
pip install -e sdk/python/
# or
uv pip install -e sdk/python/
```

## Quick Start

```python
from cogkos import CogKOS

brain = CogKOS("http://localhost:3000/mcp", api_key="xxx", tenant_id="my-project")

# Store
brain.learn("RLS requires explicit WHERE tenant_id", confidence=0.9, tags=["security"])

# Query
result = brain.recall("multi-tenant isolation")
print(result.best_belief)

# Feedback
brain.feedback(result.query_hash, success=True)

# Report gap
brain.report_gap(domain="security", description="Missing RBAC docs")

brain.close()
```

## Context Manager

```python
with CogKOS("http://localhost:3000/mcp", api_key="xxx", tenant_id="dev") as brain:
    result = brain.recall("knowledge decay")
    print(result.best_belief)
```

## API

| Method | MCP Tool | Description |
|--------|----------|-------------|
| `learn()` | `submit_experience` | Store knowledge claim |
| `recall()` | `query_knowledge` | Query knowledge base |
| `feedback()` | `submit_feedback` | Feedback on query result |
| `report_gap()` | `report_gap` | Report missing knowledge |

## Error Handling

```python
from cogkos import CogKOS, CogKOSError

try:
    result = brain.recall("test")
except CogKOSError as e:
    print(f"Error: {e}, code: {e.code}")
```

## Requirements

- Python >= 3.10
- httpx >= 0.27
- CogKOS server running with `MCP_TRANSPORT=http`
