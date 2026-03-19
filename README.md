# CogKOS — Cognitive Knowledge Operating System

[![CI](https://github.com/Kingxiao/cogkos/actions/workflows/ci.yml/badge.svg)](https://github.com/Kingxiao/cogkos/actions)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.94+-orange.svg)](https://www.rust-lang.org)

**[中文文档 / Chinese](docs/README.zh-CN.md)**

CogKOS is a **cognitive knowledge backend for AI Agents** — providing long-term memory, knowledge evolution, and prediction capabilities through the [MCP (Model Context Protocol)](https://modelcontextprotocol.io/).

> Agents make decisions. CogKOS remembers *why*.

## Key Features

| Feature | Description |
|---------|-------------|
| **Seven-Layer Architecture** | Clear separation from persistence to ingestion pipeline |
| **MCP Protocol** | Standard MCP via rmcp SDK — stdio and Streamable HTTP transports |
| **Dual-Mode Evolution** | Incremental optimization (99%) + paradigm shift (1%) |
| **Multi-Tenant Isolation** | Database-level RLS, per-tenant data isolation |
| **Sleep-time Compute** | Async knowledge consolidation, confidence decay, conflict resolution |
| **Knowledge Graph** | FalkorDB-backed graph with activation diffusion |
| **Semantic Search** | PostgreSQL pgvector with runtime dimension detection |
| **Async Write Path** | Fast-track PG insert (~1ms), background embedding + indexing (S2 principle) |

## Quick Start

### Prerequisites

- **Rust** 1.94+ (edition 2024)
- **PostgreSQL** 17+ with pgvector extension
- **FalkorDB** (Redis-protocol compatible)
- **Docker & Docker Compose** (recommended)

### 1. Clone & Start Infrastructure

```bash
git clone https://github.com/Kingxiao/cogkos.git
cd cogkos

# Start PostgreSQL + FalkorDB
docker-compose up -d
```

### 2. Configure & Run

```bash
cp .env.example .env
# Edit .env — set DATABASE_URL, FALKORDB_URL, embedding API key

cargo build --release
./target/release/cogkos
```

The MCP server starts on `http://localhost:3000/mcp`, health endpoint on `:8081/healthz`.

### 3. Connect an Agent

```json
// MCP client config (e.g., Claude Code ~/.claude/mcp_servers.json)
{
  "cogkos": {
    "type": "streamable-http",
    "url": "http://localhost:3000/mcp",
    "headers": {
      "X-API-Key": "<your-key>",
      "X-Tenant-ID": "default"
    }
  }
}
```

## Architecture

```
L7  Ingestion Pipeline — PDF/Word/Markdown parsing + LLM classification
L6  MCP Server (rmcp SDK) — Auth, caching, activation diffusion
L5  Knowledge Graph (FalkorDB) — EpistemicClaim nodes, relations, conflicts
L4  Evolution Engine — Incremental (99%) + paradigm shift (1%)
L3  Sleep-time Compute — Conflict detection, Bayesian aggregation, decay
L2  External Knowledge — RSS/Webhook/API polling
L1  Persistence — PostgreSQL + pgvector / FalkorDB / S3
```

## MCP Tools

| Tool | Description |
|------|-------------|
| `query_knowledge` | Semantic search + graph diffusion, returns structured decision envelope |
| `submit_experience` | Ingest knowledge claims with async embedding + conflict detection |
| `submit_feedback` | Feedback loop — adjusts confidence via 70/30 blending |
| `report_gap` | Report knowledge gaps for targeted acquisition |
| `upload_document` | Upload documents into the ingestion pipeline |
| `get_meta_directory` | Browse knowledge domains and expertise scores |
| `subscribe_rss` / `subscribe_webhook` / `subscribe_api` | External source subscriptions |

## Crate Structure

```
crates/
├── cogkos-core/       Core models, RBAC, evolution engine, health monitor
├── cogkos-store/      PostgreSQL + pgvector + FalkorDB + S3 storage
├── cogkos-mcp/        MCP Server, query/ingest/feedback handlers
├── cogkos-ingest/     Document parsing + vectorization pipeline
├── cogkos-sleep/      Async task scheduler (conflict, decay, aggregation)
├── cogkos-llm/        Multi-provider LLM client
├── cogkos-external/   RSS/Webhook/API polling
├── cogkos-federation/ Cross-instance routing (experimental)
└── cogkos-workflow/   Workflow engine (placeholder)
```

## Design Principles

| # | Principle | Implication |
|---|-----------|-------------|
| S1 | Memory is prediction | Query responses include predictions + confidence |
| S2 | Fast capture / slow consolidation | Write path: sync PG insert, async embedding + indexing |
| S3 | Read equals write | Queries atomically update activation weights |
| S4 | Knowledge has a shelf life | `confidence × e^(-λt)`, modulated by activation weight |
| S5 | Evolution triad | Mutation (conflict) + Selection (decay) + Inheritance (Bayesian aggregation) |
| S6 | Dual-path cognition | System 1 (cache) + System 2 (full reasoning) |

## Tech Stack

| Component | Technology |
|-----------|------------|
| Language | Rust 1.94+ (edition 2024) |
| Relational DB | PostgreSQL 17 + pgvector (HNSW index) |
| Graph DB | FalkorDB (Redis protocol) |
| Object Storage | S3 / SeaweedFS / local filesystem fallback |
| MCP | rmcp SDK 1.2+ (stdio + Streamable HTTP) |
| Observability | Prometheus + OpenTelemetry + JSON logging |

## Development

```bash
cargo test          # Unit tests (69 tests)
cargo fmt           # Format
cargo clippy        # Lint
cargo audit         # Security audit
```

## Deployment

```bash
# Docker
docker build -t cogkos:latest .
docker run -d -p 3000:3000 --env-file .env cogkos:latest

# Kubernetes
kubectl apply -k k8s/overlays/dev
```

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).

## Links

- [Architecture](docs/ARCHITECTURE.md)
- [API Spec](docs/API_SPEC.md)
- [Design Principles](docs/PRINCIPLES.md)
- [Roadmap](docs/ROADMAP.md)
