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

## Embedding Configuration

CogKOS defaults to local BGE-M3 via TEI (no API key needed):

```bash
# Start local embedding server
docker compose -f docker-compose.bge-m3.yml up -d

# Or use a cloud provider (set in .env)
EMBEDDING_MODEL=BAAI/bge-m3
EMBEDDING_BASE_URL=https://api.deepinfra.com/v1/openai
EMBEDDING_API_KEY=your_key_here
```

See the main project README for all embedding options.

## Requirements

- Python >= 3.10
- httpx >= 0.27
- CogKOS server running with `MCP_TRANSPORT=http`
