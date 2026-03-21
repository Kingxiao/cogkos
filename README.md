# CogKOS — Long-term Memory for AI Agents

[![CI](https://github.com/Kingxiao/cogkos/actions/workflows/ci.yml/badge.svg)](https://github.com/Kingxiao/cogkos/actions)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE-APACHE)
[![Rust](https://img.shields.io/badge/rust-1.94+-orange.svg)](https://www.rust-lang.org)

**[中文文档 / Chinese](docs/README.zh-CN.md)**

Give your AI agents persistent memory that works across sessions and across agents. Self-hosted, open-source, connects via [MCP](https://modelcontextprotocol.io/).

Your agents forget everything between sessions. CogKOS fixes that — it stores what they learn, finds what's relevant next time, and quietly retires knowledge that goes stale. Upload your company docs, and every agent can find what they need without you repeating yourself.

> Stop repeating yourself to your agents.

## Why CogKOS

| Problem | What CogKOS does |
|---------|-----------------|
| **Agents forget between sessions** | Persistent memory — knowledge survives restarts and new conversations |
| **Agents don't share what they learn** | One agent's discovery is available to all your agents automatically |
| **Context windows fill up with reminders** | Semantic search retrieves only what's relevant — no token waste |
| **Company docs and agent guesses treated equally** | Five authority tiers — company policy always ranks above agent speculation |
| **Notes and wikis go stale** | Confidence decays over time; contradictions get flagged automatically |
| **You don't trust cloud storage** | Runs on your machine with local BGE-M3 embedding — zero API fees |

## Quick Start

### Prerequisites

- **Rust** 1.94+ (edition 2024)
- **Docker & Docker Compose**
- **NVIDIA GPU** (optional, for faster embedding)

### 1. Start everything

```bash
git clone https://github.com/Kingxiao/cogkos.git && cd cogkos
docker-compose up -d                              # PostgreSQL + FalkorDB
docker compose -f docker-compose.bge-m3.yml up -d # Local BGE-M3 embedding (GPU)
cp .env.example .env
cargo build --release
./target/release/cogkos &
```

> No GPU? Use `docker compose -f docker-compose.bge-m3.yml --profile cpu up -d` instead.
> Set `DEFAULT_MCP_API_KEY=any-string` in `.env` for quick dev mode.

### 2. Connect your agent

Add to `~/.claude/mcp_servers.json` (Claude Code) or your agent's MCP config:

```json
{
  "cogkos": {
    "type": "streamable-http",
    "url": "http://localhost:3000/mcp",
    "headers": { "X-API-Key": "your-key" }
  }
}
```

### 3. Use it

```python
from cogkos import CogKOS

brain = CogKOS("http://localhost:3000/mcp", api_key="your-key", tenant_id="my-project")

brain.learn("Our API uses bcrypt for password hashing, never md5", confidence=0.95)
result = brain.recall("what hashing algorithm should we use?")
brain.feedback(result.query_hash, success=True)
```

Your agent automatically gets these MCP tools:

| Tool | What it does |
|------|-------------|
| `query_knowledge` | Semantic search + graph traversal — finds what's relevant |
| `submit_experience` | Store a learning, decision, or observation |
| `submit_feedback` | Tell CogKOS whether its answer was useful (tunes confidence) |
| `upload_document` | Feed documents (PDF, Word, Excel, CSV, images, and more) |
| `report_gap` | Flag missing knowledge for targeted acquisition |
| `get_meta_directory` | Browse knowledge domains and expertise scores |

## Key Features

### Knowledge Authority Tiers

Not all knowledge is created equal. CogKOS automatically classifies and prioritizes:

| Tier | What it is | Decay | Query priority |
|------|-----------|-------|---------------|
| **T1 Canonical** | Company policies, admin-uploaded docs | Never decays | Highest |
| **T2 Curated** | Uploaded reference documents | Very slow | High |
| **T3 Verified** | Agent knowledge confirmed by feedback | Slow | Medium |
| **T4 Observed** | Agent discoveries (default) | Normal | Standard |
| **T5 Ephemeral** | Working memory, RSS feeds | Fast | Lowest |

When a company policy conflicts with an agent's guess, the policy wins automatically.

### Document Ingestion

Upload documents and CogKOS extracts structured knowledge:

| Format | Support |
|--------|---------|
| PDF, Word (.docx), PowerPoint (.pptx) | Full parsing + semantic chunking |
| Excel (.xlsx), CSV, TSV | Row-based chunking with header context |
| Markdown, HTML, JSON, XML, YAML | Native text parsing |
| Images (PNG, JPG) | LLM vision-based text extraction |

Documents are split at semantic boundaries (paragraphs, sections), not fixed character windows. When an LLM is configured, key facts, decisions, and predictions are extracted as structured claims.

### Three Memory Tiers

| Tier | Scope | Lifetime | Shared? |
|------|-------|----------|---------|
| **Semantic** | Tenant-wide | Months | Yes — all agents |
| **Episodic** | Per-agent | Days | No — only that agent |
| **Working** | Per-session | Hours | No — only that session |

Default queries only search the semantic tier. An agent's scratch notes never leak into another agent's results.

## How It Works

```
L7  Ingestion    — PDF/Word/Excel/CSV/JSON/XML/HTML/Image + LLM extraction
L6  MCP Server   — Auth, caching, semantic search, graph diffusion
L5  Knowledge Graph — Claims, relations, conflict records (FalkorDB)
L4  Evolution    — Authority-aware decay, Bayesian aggregation, conflict resolution
L3  Background   — 14 scheduled tasks with circuit breakers
L2  External     — RSS/Webhook/API polling for outside knowledge
L1  Storage      — PostgreSQL + pgvector / FalkorDB / S3
```

## Embedding Model

CogKOS supports any OpenAI-compatible embedding API. Local BGE-M3 is the default — no API key needed.

| Model | Dimensions | Cost | Setup |
|-------|-----------|------|-------|
| **BGE-M3 (local GPU)** | 1024 | Free | `docker compose -f docker-compose.bge-m3.yml up -d` |
| BGE-M3 (local CPU) | 1024 | Free | `docker compose -f docker-compose.bge-m3.yml --profile cpu up -d` |
| BGE-M3 (DeepInfra) | 1024 | ~$0.01/1M tokens | Set `EMBEDDING_API_KEY` in `.env` |
| text-embedding-3-large | 3072 | ~$0.13/1M tokens | Set `OPENAI_API_KEY` in `.env` |

## Python SDK

```bash
pip install cogkos  # or: cd sdk/python && pip install -e .
```

```python
from cogkos import CogKOS

brain = CogKOS("http://localhost:3000/mcp", api_key="key", tenant_id="my-project")
brain.learn("Rust uses borrow checker for memory safety", confidence=0.9)
result = brain.recall("how does Rust handle memory?")
print(result.best_belief)
```

## Project Structure

```
crates/
├── cogkos-core/       Data models, authority tiers, RBAC, evolution engine
├── cogkos-store/      PostgreSQL + pgvector + FalkorDB + S3 storage
├── cogkos-mcp/        MCP server, query/ingest/feedback handlers
├── cogkos-ingest/     Document parsing, semantic chunking, LLM extraction
├── cogkos-sleep/      14-task background scheduler with circuit breakers
├── cogkos-llm/        Multi-provider LLM client
├── cogkos-external/   RSS/Webhook/API polling
└── cogkos-federation/ Collective wisdom health checks (partial)
sdk/python/            Python SDK
```

## Development

```bash
cargo test          # 600+ tests
cargo fmt           # Format
cargo clippy        # Lint
```

## Deployment

```bash
# Docker (one-click)
docker compose -f docker-compose.quickstart.yml up -d

# Production
docker build -t cogkos:latest .
docker run -d -p 3000:3000 -p 8081:8081 --env-file .env cogkos:latest

# Health checks
curl http://localhost:8081/healthz   # → "ok"
curl http://localhost:8081/readyz    # → "ready"
```

## License

Licensed under [Apache-2.0](LICENSE-APACHE).

## Links

- [Architecture](docs/ARCHITECTURE.md)
- [API Spec](docs/API_SPEC.md)
- [Design Principles](docs/PRINCIPLES.md)
- [Roadmap](docs/ROADMAP.md)
