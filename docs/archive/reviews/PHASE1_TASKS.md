# Phase 1 实施任务拆解

## 目标

系统能跑起来。Agent 能通过 MCP 查询和写入知识。

## 交付标准

- [ ] MCP Server 启动，Agent 可通过 stdio/SSE 连接
- [ ] `query_knowledge` 返回结构化 McpQueryResponse
- [ ] `submit_experience` 写入 EpistemicClaim
- [ ] `submit_feedback` 记录反馈
- [ ] `upload_document` 支持 PDF/Word/Markdown 摄入
- [ ] 多租户数据隔离
- [ ] 集成测试通过

## 依赖关系

```
Step 1 (脚手架)
  ↓
Step 2 (核心模型) ← 不依赖数据库
  ↓
Step 3 (存储层) ← 需要 Docker 环境
  ↓
Step 4 (摄入管道)
  ↓
Step 5 (MCP Server)
  ↓
Step 6 (集成测试)
```

---

## Step 1: 项目脚手架（预计 0.5 天）

### 1.1 初始化 Cargo Workspace

```bash
mkdir cogkos && cd cogkos
cargo init --lib crates/cogkos-core
cargo init --lib crates/cogkos-store
cargo init --lib crates/cogkos-mcp
cargo init --lib crates/cogkos-ingest
cargo init --lib crates/cogkos-sleep
cargo init .
```

按 `PROJECT_SETUP.md` 配置 workspace Cargo.toml。

### 1.2 创建 Docker Compose

复制 `PROJECT_SETUP.md` 中的 `docker-compose.yml`。

### 1.3 创建配置文件

- `config/default.toml`
- `.env.example`
- `.gitignore`

### 1.4 验证

```bash
docker compose up -d
cargo build --workspace
# 所有 crate 编译通过
```

---

## Step 2: 核心数据模型 — cogkos-core（预计 1 天）

### 2.1 数据结构定义

**文件**：`crates/cogkos-core/src/models/`

按 `DATA_MODELS.md` 实现：

| 文件 | 结构体 |
|------|--------|
| `claim.rs` | `EpistemicClaim`, `NodeType`, `Claimant`, `ProvenanceRecord`, `ConsolidationStage`, `EpistemicStatus` |
| `conflict.rs` | `ConflictRecord`, `ConflictType` |
| `access.rs` | `AccessEnvelope` |
| `query.rs` | `McpQueryResponse`, `QueryCacheEntry` |
| `feedback.rs` | `AgentFeedback` |
| `evolution.rs` | `EvolutionEngineState`, `EvolutionMode`, `ShiftRecord` |
| `subscription.rs` | `SubscriptionSource`, `SubscriptionType` |
| `mod.rs` | 统一导出 |

### 2.2 错误类型

**文件**：`crates/cogkos-core/src/errors.rs`

按 `PROJECT_SETUP.md` 中的 `CogKosError` 实现。

### 2.3 领域逻辑

**文件**：`crates/cogkos-core/src/evolution/`

| 文件 | 函数 |
|------|------|
| `bayesian.rs` | `bayesian_aggregate(claims) -> f64` |
| `decay.rs` | `calculate_decay(confidence, lambda, time_delta, activation_weight) -> f64` |
| `conflict.rs` | `detect_conflict(claim_a, claim_b) -> Option<ConflictRecord>` |
| `activation.rs` | `should_update_activation(sample_rate) -> bool` |

### 2.4 验证

```bash
cargo test -p cogkos-core
# 单元测试：贝叶斯聚合、衰减公式、冲突检测逻辑
```

---

## Step 3: 存储层 — cogkos-store（预计 2 天）

### 3.1 SQL 迁移

**文件**：`migrations/001_init.sql`

按 `DATA_MODELS.md` 的 SQL Schema 实现。包含：
- `epistemic_claims` 表
- `conflict_records` 表
- `query_cache` 表
- `agent_feedback` 表
- `subscription_sources` 表
- `evolution_state` 表
- `api_keys` 表
- 索引

### 3.2 PostgreSQL 存储

**文件**：`crates/cogkos-store/src/postgres.rs`

实现 trait：

```rust
#[async_trait]
pub trait ClaimStore: Send + Sync {
    async fn insert_claim(&self, claim: &EpistemicClaim) -> Result<Uuid>;
    async fn get_claim(&self, id: Uuid, tenant_id: &str) -> Result<EpistemicClaim>;
    async fn update_activation(&self, id: Uuid, delta: f64) -> Result<()>;
    async fn batch_update_activation(&self, updates: &[(Uuid, f64)]) -> Result<()>;
    async fn list_claims_by_stage(
        &self, tenant_id: &str, stage: ConsolidationStage, limit: u32
    ) -> Result<Vec<EpistemicClaim>>;
    async fn insert_conflict(&self, conflict: &ConflictRecord) -> Result<Uuid>;
    async fn get_conflicts_for_claim(&self, claim_id: Uuid) -> Result<Vec<ConflictRecord>>;
}

#[async_trait]
pub trait CacheStore: Send + Sync {
    async fn get_cached(&self, query_hash: u64) -> Result<Option<QueryCacheEntry>>;
    async fn set_cached(&self, entry: &QueryCacheEntry) -> Result<()>;
    async fn invalidate_by_claim(&self, claim_id: Uuid) -> Result<u32>;
}

#[async_trait]
pub trait FeedbackStore: Send + Sync {
    async fn insert_feedback(&self, feedback: &AgentFeedback) -> Result<()>;
}

#[async_trait]
pub trait AuthStore: Send + Sync {
    async fn validate_api_key(&self, key: &str) -> Result<(String, Vec<String>)>;
    // 返回 (tenant_id, permissions)
}
```

### 3.3 FalkorDB 图存储

**文件**：`crates/cogkos-store/src/graph.rs`

```rust
#[async_trait]
pub trait GraphStore: Send + Sync {
    async fn upsert_node(&self, claim: &EpistemicClaim) -> Result<()>;
    async fn create_edge(
        &self, from: Uuid, to: Uuid, relation: &str, weight: f64
    ) -> Result<()>;
    async fn find_related(
        &self, claim_id: Uuid, max_depth: u32, min_activation: f64
    ) -> Result<Vec<GraphNode>>;
    async fn detect_semantic_conflicts(
        &self, claim: &EpistemicClaim
    ) -> Result<Vec<ConflictRecord>>;
}
```

FalkorDB Cypher 查询示例：

```cypher
// 查找关联节点（激活扩散简化版 Phase 1）
MATCH (a:Claim {id: $claim_id})-[r*1..2]-(b:Claim)
WHERE b.tenant_id = $tenant_id
RETURN b, r, length(r) as depth
ORDER BY b.confidence DESC
LIMIT $limit
```

### 3.4 Qdrant 向量存储

**文件**：`crates/cogkos-store/src/vector.rs`

```rust
#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn upsert(&self, id: Uuid, vector: Vec<f32>, payload: Value) -> Result<()>;
    async fn search(
        &self, query_vector: Vec<f32>, tenant_id: &str, limit: u32
    ) -> Result<Vec<VectorMatch>>;
    async fn calculate_novelty(
        &self, vector: Vec<f32>, tenant_id: &str
    ) -> Result<f64>;
    // 返回与最近邻的距离，越大越新颖
}
```

### 3.5 S3 存储

**文件**：`crates/cogkos-store/src/s3.rs`

```rust
#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn upload(&self, key: &str, data: &[u8], content_type: &str) -> Result<String>;
    async fn download(&self, key: &str) -> Result<Vec<u8>>;
    async fn delete(&self, key: &str) -> Result<()>;
}
```

### 3.6 验证

```bash
docker compose up -d
cargo test -p cogkos-store
# 集成测试：对真实 PG/FalkorDB/Qdrant/MinIO 的 CRUD
```

---

## Step 4: 摄入管道 — cogkos-ingest（预计 1.5 天）

### 4.1 格式解析器

**文件**：`crates/cogkos-ingest/src/parser/`

| 文件 | 解析 | 依赖 |
|------|------|------|
| `markdown.rs` | Markdown → 分段文本 | `pulldown-cmark` |
| `pdf.rs` | PDF → 文本 | `pdf-extract` 或调用外部 `pdftotext` |
| `docx.rs` | Word → 文本 | `docx-rs` |
| `txt.rs` | 纯文本 → 分段 | 无 |

统一 trait：

```rust
pub trait DocumentParser: Send + Sync {
    fn supported_extensions(&self) -> &[&str];
    fn parse(&self, data: &[u8], filename: &str) -> Result<Vec<TextChunk>>;
}

pub struct TextChunk {
    pub content: String,
    pub chunk_index: u32,
    pub metadata: HashMap<String, String>,
}
```

### 4.2 粗分类器

**文件**：`crates/cogkos-ingest/src/classifier.rs`

```rust
pub fn coarse_classify(filename: &str, format: &str) -> CoarseClassification {
    // 文件名解析："XX公司2025年报.pdf" → Entity=XX公司, Type=年报
    // 格式推断 → NodeType
    // 返回粗分类结果（无 LLM）
}
```

### 4.3 摄入管道编排

**文件**：`crates/cogkos-ingest/src/pipeline.rs`

```rust
pub async fn ingest_document(
    file: UploadedFile,
    stores: &Stores,
    embedding: &EmbeddingService,
) -> Result<IngestResult> {
    // 1. 存入 S3
    // 2. 格式检测 → 选择解析器
    // 3. 解析 → TextChunk[]
    // 4. 每个 chunk：
    //    a. 创建 EpistemicClaim (FastTrack)
    //    b. 向量化 (fastembed)
    //    c. 写入 Qdrant
    //    d. 写入 FalkorDB 节点
    //    e. 语义距离计算（新颖度）
    //    f. 冲突检测
    // 5. 返回 IngestResult { claim_ids, conflicts_detected }
}
```

### 4.4 嵌入服务

**文件**：`crates/cogkos-ingest/src/embedding.rs`

```rust
pub struct EmbeddingService {
    model: fastembed::TextEmbedding,
}

impl EmbeddingService {
    pub fn new(model_name: &str) -> Result<Self>;
    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
}
```

### 4.5 验证

```bash
cargo test -p cogkos-ingest
# 测试：解析各格式文件 → 生成 TextChunk → 端到端写入
```

---

## Step 5: MCP Server — cogkos-mcp（预计 2 天）

### 5.1 Server 框架

**文件**：`crates/cogkos-mcp/src/server.rs`

JSON-RPC 2.0 处理：

```rust
pub struct McpServer {
    stores: Arc<Stores>,
    embedding: Arc<EmbeddingService>,
    cache: Arc<RwLock<LruCache<u64, QueryCacheEntry>>>,
    config: Config,
}

impl McpServer {
    pub async fn start(config: Config) -> Result<()>;
    async fn handle_request(&self, req: JsonRpcRequest) -> JsonRpcResponse;
    fn register_tools(&self) -> Vec<ToolDefinition>;
}
```

支持两种传输：
- `--transport stdio`：标准输入输出
- `--transport sse --port 3000`：HTTP SSE

### 5.2 工具实现

**文件**：`crates/cogkos-mcp/src/tools/`

按 `API_SPEC.md` 中的 6 个工具实现：

| 文件 | 工具 | 核心逻辑 |
|------|------|---------|
| `query.rs` | `query_knowledge` | 缓存查找 → Qdrant 检索 → FalkorDB 关联 → 合并排序 → 响应 |
| `submit.rs` | `submit_experience` | 校验 → 创建 Claim → 向量化 → 写入各存储 → 冲突检测 |
| `feedback.rs` | `submit_feedback` | 校验 → 更新缓存统计 → 记录反馈 |
| `gap.rs` | `report_gap` | 校验 → 记录知识空洞 |
| `meta.rs` | `get_meta_directory` | 查询元知识索引 |
| `upload.rs` | `upload_document` | 校验 → 调 cogkos-ingest 管道 |

### 5.3 鉴权中间件

**文件**：`crates/cogkos-mcp/src/auth.rs`

```rust
pub async fn authenticate(
    api_key: &str,
    auth_store: &dyn AuthStore,
    cache: &ApiKeyCache,
) -> Result<AuthContext> {
    // 1. 查缓存
    // 2. 缓存未命中 → 查数据库
    // 3. 校验权限
    // 4. 返回 AuthContext { tenant_id, permissions }
}
```

### 5.4 查询缓存

**文件**：`crates/cogkos-mcp/src/cache.rs`

按 `ARCHITECTURE.md` L6 查询缓存设计实现。

### 5.5 主入口

**文件**：`src/main.rs`

```rust
#[tokio::main]
async fn main() -> Result<()> {
    // 1. 加载配置
    // 2. 初始化 tracing
    // 3. 初始化存储连接
    // 4. 运行 SQL 迁移
    // 5. 初始化嵌入模型
    // 6. 启动 MCP Server
    // 7. 启动 Sleep-time 调度器（后台任务）
}
```

### 5.6 验证

```bash
# 启动服务
docker compose up -d
cargo run -- --transport sse --port 3000

# 手动测试（curl）
curl -X POST http://localhost:3000/mcp \
  -H "Content-Type: application/json" \
  -H "X-API-Key: test-key" \
  -H "X-Tenant-ID: test-tenant" \
  -d '{"jsonrpc":"2.0","method":"tools/list","params":{},"id":"1"}'
```

---

## Step 6: Sleep-time 基础 + 集成测试（预计 1 天）

### 6.1 Sleep-time 调度器

**文件**：`crates/cogkos-sleep/src/scheduler.rs`

Phase 1 只需要 3 个任务：

```rust
pub async fn start_scheduler(stores: Arc<Stores>) {
    // 冲突检测：每次写入后事件驱动
    // 贝叶斯聚合：每 6 小时
    // 知识衰减：每 24 小时
}
```

### 6.2 端到端集成测试

**文件**：`tests/integration/`

| 测试文件 | 测试内容 |
|---------|---------|
| `test_write_read.rs` | submit_experience → query_knowledge → 验证返回 |
| `test_feedback.rs` | query → feedback → 验证缓存更新 |
| `test_upload.rs` | upload_document → 等待摄入 → query 验证知识提取 |
| `test_auth.rs` | 无 Key / 错误 Key / 正确 Key 的鉴权 |
| `test_multitenancy.rs` | 租户 A 写入 → 租户 B 查不到 |

### 6.3 验证

```bash
docker compose up -d
cargo test --workspace
# 全部通过 = Phase 1 完成
```

---

## 时间线总览

| Step | 内容 | 预计时间 | 依赖 |
|------|------|---------|------|
| 1 | 项目脚手架 | 0.5 天 | 无 |
| 2 | 核心数据模型 | 1 天 | Step 1 |
| 3 | 存储层 | 2 天 | Step 2 |
| 4 | 摄入管道 | 1.5 天 | Step 3 |
| 5 | MCP Server | 2 天 | Step 3, 4 |
| 6 | Sleep-time + 集成测试 | 1 天 | Step 5 |
| **总计** | | **~8 天** | |
