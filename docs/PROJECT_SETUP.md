# 项目工程规范

## Cargo Workspace 布局

```
cogkos/
├── Cargo.toml                  # workspace root
├── docker-compose.yml          # 本地开发环境
├── .env.example                # 环境变量模板
├── config/
│   └── default.toml            # 默认配置
│
├── crates/
│   ├── cogkos-core/            # 核心数据结构 + 领域逻辑
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── models/         # EpistemicClaim, ConflictRecord, AccessEnvelope...
│   │       ├── evolution/      # 进化引擎（贝叶斯聚合、衰减、冲突检测）
│   │       └── errors.rs       # 错误类型
│   │
│   ├── cogkos-store/           # 存储抽象层
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── postgres.rs     # PostgreSQL 实现
│   │       ├── graph.rs        # FalkorDB 实现
│   │       ├── vector.rs       # pgvector 实现
│   │       └── s3.rs           # S3 实现
│   │
│   ├── cogkos-mcp/             # MCP Server 实现
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── server.rs       # JSON-RPC 2.0 server
│   │       ├── tools/          # 各 MCP 工具实现
│   │       │   ├── query.rs
│   │       │   ├── submit.rs
│   │       │   ├── feedback.rs
│   │       │   └── meta.rs
│   │       ├── cache.rs        # 查询缓存
│   │       └── auth.rs         # 鉴权
│   │
│   ├── cogkos-ingest/          # 摄入管道
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── parser/         # 多格式解析器
│   │       │   ├── markdown.rs
│   │       │   ├── pdf.rs
│   │       │   └── docx.rs
│   │       ├── classifier.rs   # 粗分类
│   │       └── pipeline.rs     # 摄入管道编排
│   │
│   ├── cogkos-sleep/           # Sleep-time 异步任务
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── scheduler.rs    # 任务调度
│   │       ├── consolidate.rs  # 贝叶斯聚合
│   │       ├── decay.rs        # 知识衰减
│   │       └── conflict.rs     # 冲突检测
│   │
│   ├── cogkos-llm/             # 多供应商 LLM 客户端（Anthropic/OpenAI 兼容）
│   │   ├── Cargo.toml
│   │   └── src/
│   │
│   ├── cogkos-external/        # 外部知识源（RSS/Webhook/API 轮询）
│   │   ├── Cargo.toml
│   │   └── src/
│   │
│   └── cogkos-federation/      # 联邦层（群体智慧健康检查已激活，跨实例路由冻结）
│       ├── Cargo.toml
│       └── src/
│
├── src/
│   └── main.rs                 # 二进制入口（启动 MCP Server + Sleep-time）
│
├── migrations/                 # SQL 迁移文件
│   └── 001_init.sql
│
└── tests/
    ├── integration/            # 集成测试
    └── fixtures/               # 测试数据
```

## Workspace Cargo.toml

```toml
[workspace]
resolver = "2"
members = [
    "crates/cogkos-core",
    "crates/cogkos-store",
    "crates/cogkos-mcp",
    "crates/cogkos-ingest",
    "crates/cogkos-sleep",
    "crates/cogkos-llm",
    "crates/cogkos-external",
    "crates/cogkos-federation",
]

[workspace.dependencies]
# 异步运行时
tokio = { version = "1", features = ["full"] }

# 序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# 数据库
sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "uuid", "chrono", "json"] }
# 注: 若升级 sqlx 到 0.8+，注意 Transaction Executor breaking change: 需使用 `&mut *tx`
deadpool-redis = "0.18"           # FalkorDB (Redis 协议)

# 向量库（pgvector 通过 sqlx 访问，无需独立客户端）

# 对象存储
aws-sdk-s3 = "1"

# HTTP / MCP
axum = "0.8"
tower = "0.5"
rmcp = { version = "0.1", features = ["server", "macros", "transport-io", "transport-streamable-http-server"] }

# ID
uuid = { version = "1", features = ["v4", "serde"] }

# 时间
chrono = { version = "0.4", features = ["serde"] }

# 日志
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["json", "env-filter"] }

# 配置
config = "0.14"

# 错误处理
thiserror = "2"
anyhow = "1"

# 嵌入向量（BGE-M3 默认，支持本地 TEI / DeepInfra / OpenAI 兼容端点）
fastembed = "5"

# 测试
tokio-test = "0.4"
```

## Docker Compose（本地开发环境）

```yaml
# docker-compose.yml
services:
  postgres:
    image: pgvector/pgvector:pg16  # 内置 pgvector 扩展
    ports: ["5432:5432"]
    environment:
      POSTGRES_USER: cogkos
      POSTGRES_PASSWORD: cogkos_dev
      POSTGRES_DB: cogkos
    volumes:
      - ./data/postgres:/var/lib/postgresql/data
      - ./migrations:/docker-entrypoint-initdb.d

  falkordb:
    image: falkordb/falkordb:latest
    ports: ["6379:6379"]
    volumes:
      - ./data/falkordb:/data

  minio:
    image: minio/minio:latest
    ports: ["9000:9000", "9001:9001"]
    command: server /data --console-address ":9001"
    environment:
      MINIO_ROOT_USER: cogkos
      MINIO_ROOT_PASSWORD: cogkos_dev
    volumes:
      - ./data/minio:/data
```

> 数据存储在项目目录 `./data/` 下，`.gitignore` 中已排除。容器删除/重建不丢数据。

> **pgvector 初始化**：首次启动后需执行 `CREATE EXTENSION IF NOT EXISTS vector;`（或在 migration 中包含）。

## 环境变量

```bash
# .env.example

# PostgreSQL
DATABASE_URL=postgres://cogkos:cogkos_dev@localhost:5432/cogkos

# FalkorDB
FALKORDB_URL=redis://localhost:6379

# pgvector 通过 DATABASE_URL 访问，无需独立配置

# S3 (MinIO)
S3_ENDPOINT=http://localhost:9000
S3_ACCESS_KEY=cogkos
S3_SECRET_KEY=cogkos_dev
S3_BUCKET=cogkos-docs
S3_REGION=us-east-1

# MCP Server
MCP_HOST=0.0.0.0
MCP_PORT=3000

# 嵌入模型（维度运行时自动检测，无需手动指定）
EMBEDDING_MODEL=BAAI/bge-m3  # 1024d, or text-embedding-3-large for 3072d

# 日志
RUST_LOG=cogkos=debug,tower=info

# LLM (Phase 2+)
LLM_API_URL=
LLM_API_KEY=
LLM_MODEL=
```

## 配置文件

```toml
# config/default.toml

[server]
host = "0.0.0.0"
port = 3000
max_connections = 10000

[cache]
enabled = true
ttl_seconds = 3600
max_entries = 10000

[evolution]
conflict_detection_enabled = true
decay_interval_hours = 24
consolidation_interval_hours = 6

[activation]
sample_rate = 1.0           # V1: 100%, V2: 0.1, V3: 0.05
batch_flush_interval_ms = 5000
batch_max_size = 1000

[ingest]
max_file_size_mb = 500
supported_formats = ["pdf", "docx", "md", "txt", "xlsx", "pptx"]
```

## 错误处理规范

```rust
// crates/cogkos-core/src/errors.rs

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CogKosError {
    // 客户端错误 (4xx)
    #[error("未找到: {0}")]
    NotFound(String),

    #[error("权限不足: {0}")]
    Forbidden(String),

    #[error("参数无效: {0}")]
    InvalidInput(String),

    #[error("租户不存在: {0}")]
    TenantNotFound(String),

    // 服务端错误 (5xx)
    #[error("数据库错误: {0}")]
    Database(#[from] sqlx::Error),

    #[error("图数据库错误: {0}")]
    Graph(String),

    #[error("向量库错误: {0}")]
    Vector(String),

    #[error("存储错误: {0}")]
    Storage(String),

    #[error("序列化错误: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("内部错误: {0}")]
    Internal(String),
}

impl CogKosError {
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "NOT_FOUND",
            Self::Forbidden(_) => "FORBIDDEN",
            Self::InvalidInput(_) => "INVALID_INPUT",
            Self::TenantNotFound(_) => "TENANT_NOT_FOUND",
            Self::Database(_) => "DATABASE_ERROR",
            Self::Graph(_) => "GRAPH_ERROR",
            Self::Vector(_) => "VECTOR_ERROR",
            Self::Storage(_) => "STORAGE_ERROR",
            Self::Serialization(_) => "SERIALIZATION_ERROR",
            Self::Internal(_) => "INTERNAL_ERROR",
        }
    }

    pub fn status_code(&self) -> u16 {
        match self {
            Self::NotFound(_) => 404,
            Self::Forbidden(_) => 403,
            Self::InvalidInput(_) | Self::TenantNotFound(_) => 400,
            _ => 500,
        }
    }
}
```

## 日志规范

```rust
// 使用 tracing，结构化 JSON 日志
// 每个请求带 request_id + tenant_id span

use tracing::{info, warn, error, instrument};

#[instrument(skip(store), fields(tenant_id = %req.tenant_id))]
async fn handle_query(req: QueryRequest, store: &Store) -> Result<QueryResponse> {
    info!(query = %req.context, "MCP query received");
    // ...
    if conflicts.len() > 0 {
        warn!(count = conflicts.len(), "Conflicts detected in query result");
    }
    Ok(response)
}
```

## 测试策略

| 层级 | 范围 | 工具 |
|------|------|------|
| **单元测试** | cogkos-core 的领域逻辑（贝叶斯聚合、衰减公式、冲突检测） | `cargo test` |
| **集成测试** | cogkos-store 对真实数据库的 CRUD | `cargo test` + Docker |
| **端到端测试** | MCP 工具的完整请求→响应→副作用 | `cargo test` + Docker Compose |
| **性能基准** | 查询延迟 P50/P99、写入吞吐、缓存命中率 | `criterion` |

```bash
# 运行全部测试
docker compose up -d
cargo test --workspace

# 仅单元测试（不需要数据库）
cargo test --workspace --lib

# 集成测试
cargo test --workspace --test '*'
```

## 安全/鉴权

Phase 1 鉴权方案（简单有效）：

```
Agent → MCP 请求 (Header: X-API-Key)
  → Gateway 校验 API Key（数据库查询，结果缓存 5 分钟）
  → 从 API Key 绑定关系提取 tenant_id → 注入到所有后续数据库查询的 WHERE 条件中
```

```sql
-- API Key 表
CREATE TABLE api_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    key_hash TEXT NOT NULL UNIQUE,   -- bcrypt hash
    tenant_id TEXT NOT NULL,
    name TEXT NOT NULL,
    permissions JSONB DEFAULT '["read", "write"]',
    created_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
    enabled BOOLEAN DEFAULT TRUE
);
```

Phase 3+ 升级到 mTLS 或 JWT。
