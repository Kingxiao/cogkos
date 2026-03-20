# CogKOS — Long-term Memory for AI Agents

[![CI](https://github.com/Kingxiao/cogkos/actions/workflows/ci.yml/badge.svg)](https://github.com/Kingxiao/cogkos/actions)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE-APACHE)
[![Rust](https://img.shields.io/badge/rust-1.94+-orange.svg)](https://www.rust-lang.org)

**[中文文档 / Chinese](docs/README.zh-CN.md)**

Give your AI agents persistent memory that works across sessions and across agents. Self-hosted, open-source, connects via [MCP](https://modelcontextprotocol.io/).

Your agents forget everything between sessions. CogKOS fixes that — it stores what they learn, finds what's relevant next time, and quietly retires knowledge that goes stale.

> Stop repeating yourself to your agents.

## Why CogKOS

| Problem | What CogKOS does |
|---------|-----------------|
| **Agents forget between sessions** | Persistent memory — knowledge survives restarts and new conversations |
| **Agents don't share what they learn** | One agent's discovery is available to all your agents automatically |
| **Context windows fill up with reminders** | Semantic search retrieves only what's relevant — no token waste |
| **Notes and wikis go stale** | Confidence decays over time; contradictions get flagged automatically |
| **You don't trust cloud storage** | Runs on your machine. Your data never leaves your infrastructure |
| **Scratch notes leak into long-term memory** | Three tiers — session scratch, per-agent experience, shared long-term |

## Quick Start

### Prerequisites

- **Rust** 1.94+ (edition 2024)
- **Docker & Docker Compose**

### 1. Start infrastructure & build

```bash
git clone https://github.com/Kingxiao/cogkos.git && cd cogkos
docker-compose up -d        # PostgreSQL (pgvector) + FalkorDB
cp .env.example .env        # Edit if needed
cargo build --release
./target/release/cogkos &   # Runs DB migrations automatically
```

> Set `DEFAULT_MCP_API_KEY=any-string` in `.env` for quick dev mode (skips API key creation).

### 2. Connect your agent

Add to your agent's MCP config (e.g. `~/.claude/mcp_servers.json` for Claude Code):

```json
{
  "cogkos": {
    "type": "streamable-http",
    "url": "http://localhost:3000/mcp",
    "headers": {
      "X-API-Key": "your-key"
    }
  }
}
```

### 3. Use it

Your agent automatically gets these MCP tools:

| Tool | What it does |
|------|-------------|
| `query_knowledge` | Retrieve relevant knowledge — semantic search + graph traversal |
| `submit_experience` | Store a learning, decision, or observation |
| `submit_feedback` | Tell CogKOS whether its answer was useful (tunes confidence) |
| `report_gap` | Flag missing knowledge for targeted acquisition |
| `upload_document` | Feed documents into the ingestion pipeline |
| `get_meta_directory` | Browse knowledge domains and expertise scores |

What they learn in this session is still there next session.

## How It Works

```
L7  Ingestion    — PDF/Word/Markdown parsing + LLM classification
L6  MCP Server   — Auth, caching, semantic search, graph diffusion
L5  Knowledge Graph — Claims, relations, conflict records (FalkorDB)
L4  Evolution    — Confidence decay, Bayesian aggregation, conflict resolution
L3  Background   — Async embedding, consolidation, garbage collection
L2  External     — RSS/Webhook/API polling for outside knowledge
L1  Storage      — PostgreSQL + pgvector / FalkorDB / S3
```

**Key concepts:**

- **EpistemicClaim** — the atomic unit of knowledge. Has content, confidence, source, and activation weight.
- **Three memory tiers** — Working (session scratch, auto-expires), Episodic (per-agent experience), Semantic (shared long-term knowledge). Queries default to the semantic tier; working memory never leaks into other agents' results.
- **Confidence decay** — `confidence × e^(-λt)`, modulated by how often the knowledge gets used. Stale knowledge fades; frequently accessed knowledge stays strong.
- **Conflict detection** — when two claims contradict, CogKOS flags the conflict for resolution rather than silently picking one.

## Project Structure

```
crates/
├── cogkos-core/       Data models, RBAC, health monitoring
├── cogkos-store/      PostgreSQL + pgvector + FalkorDB + S3 storage
├── cogkos-mcp/        MCP server, query/ingest/feedback handlers
├── cogkos-ingest/     Document parsing + vectorization pipeline
├── cogkos-sleep/      Background task scheduler (decay, aggregation)
├── cogkos-llm/        Multi-provider LLM client
├── cogkos-external/   RSS/Webhook/API polling
└── cogkos-federation/ Cross-instance routing (experimental)
```

## Configuration

Key environment variables (see `.env.example` for full list):

| Variable | Purpose |
|----------|---------|
| `DATABASE_URL` | PostgreSQL connection string |
| `FALKORDB_URL` | FalkorDB (Redis-protocol) connection |
| `API_302_KEY` or `OPENAI_API_KEY` | Embedding provider for semantic search |
| `DEFAULT_MCP_API_KEY` | Skip API key creation for local dev |
| `MCP_TRANSPORT` | `http` for Streamable HTTP (default: stdio) |

## Development

```bash
cargo test          # 69 tests
cargo fmt           # Format
cargo clippy        # Lint
```

## Deployment

```bash
# Docker
docker build -t cogkos:latest .
docker run -d -p 3000:3000 -p 8081:8081 --env-file .env cogkos:latest

# Kubernetes
kubectl apply -k k8s/overlays/dev

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
