# CogKOS 数据模型

## 核心数据结构

### EpistemicClaim — 统一知识原子

```rust
pub struct EpistemicClaim {
    pub id: Uuid,
    pub content: String,
    pub node_type: NodeType,
    pub claimant: Claimant,

    // 状态
    pub epistemic_status: EpistemicStatus,
    pub confidence: f64,

    // 生命周期
    pub consolidation_stage: ConsolidationStage,
    pub durability: f64,

    // 活跃度（S3 读即写 — 每次查询命中时原子更新）
    pub activation_weight: f64,     // 活跃权重（命中时 +Δ，衰减时 ×decay）
    pub access_count: u64,          // 总被检索次数
    pub last_accessed: Option<DateTime>, // 最后被检索时间

    // 双时态
    pub t_valid_start: DateTime,
    pub t_valid_end: Option<DateTime>,
    pub t_known: DateTime,

    // 权限
    pub access_envelope: AccessEnvelope,

    // 来源
    pub provenance: ProvenanceRecord,

    // 向量
    pub vector_id: Option<Uuid>,

    // 预测
    pub last_prediction_error: Option<f64>,

    // 进化
    pub derived_from: Vec<Uuid>,
    pub needs_revalidation: bool,
}
```

### 枚举类型

```rust
pub enum NodeType {
    Entity, Relation, Event, Attribute,
    Prediction, Insight, File,
}

pub enum ConsolidationStage {
    FastTrack,          // 断言层
    PendingAggregation, // 待聚合（代码已实现）
    Consolidated,       // 信念层
    Insight,       // 洞察层
    Archived,      // 归档
}

pub enum EpistemicStatus {
    Asserted, Corroborated, Contested,
    Retracted, Superseded,
}

pub enum Claimant {
    Human { user_id: String, role: String },
    Agent { agent_id: String, model: String },
    System,
    ExternalPublic { source_name: String },
}
```

---

### ConflictRecord

```rust
pub struct ConflictRecord {
    pub id: Uuid,
    pub claim_a_id: Uuid,
    pub claim_b_id: Uuid,
    pub conflict_type: ConflictType,
    pub detected_at: DateTime,
    pub resolution_status: ResolutionStatus,
    pub resolution_note: Option<String>,
    pub elevated_insight_id: Option<Uuid>,
}

pub enum ConflictType {
    DirectContradiction, ContextDependent,
    TemporalShift, SourceDisagreement,
    TemporalInconsistency,   // 时间不一致（代码已实现）
    ConfidenceMismatch,      // 置信度不匹配（代码已实现）
    ContextualDifference,    // 上下文差异（代码已实现）
}

pub enum ResolutionStatus {
    Open, Elevated, Dismissed, Accepted,
}
```

---

### AccessEnvelope / ProvenanceRecord

```rust
pub struct AccessEnvelope {
    pub visibility: Visibility,
    pub tenant_id: String,
    pub allowed_roles: Vec<String>,
    pub gdpr_applicable: bool,
}

pub enum Visibility {
    Private, Team, Tenant, CrossTenant, Public,
}

pub struct ProvenanceRecord {
    pub source_id: String,
    pub source_type: String,
    pub ingestion_method: String,
    pub original_url: Option<String>,
    pub audit_hash: String,
}
```

---

## 查询层类型（L6 输出）

### MCP 响应

```rust
pub struct McpQueryResponse {
    pub query_context: String,
    pub best_belief: Option<BeliefSummary>,
    pub related_by_graph: Vec<GraphRelation>,  // 激活扩散发现的间接关联
    pub conflicts: Vec<ConflictSummary>,
    pub prediction: Option<PredictionResult>,
    pub knowledge_gaps: Vec<String>,
    pub freshness: FreshnessInfo,
    pub cache_status: CacheStatus,             // 来自缓存还是完整推理
}

pub struct BeliefSummary {
    pub content: String,
    pub confidence: f64,
    pub based_on: usize,
    pub consolidation_stage: ConsolidationStage,
    pub claim_ids: Vec<Uuid>,
}

pub struct GraphRelation {
    pub content: String,
    pub relation_type: String,    // CAUSES / SIMILAR_TO / etc.
    pub activation: f64,          // 激活扩散强度
    pub source_claim_id: Uuid,
}

pub struct PredictionResult {
    pub content: String,
    pub confidence: f64,
    pub method: PredictionMethod,
    pub based_on_claims: Vec<Uuid>,
}

pub enum PredictionMethod {
    LlmBeliefContext, DedicatedModel, StatisticalTrend,
}

pub enum CacheStatus { Hit, Miss }
```

### 查询缓存条目

```rust
pub struct QueryCacheEntry {
    pub query_hash: u64,
    pub response: McpQueryResponse,
    pub confidence: f64,
    pub hit_count: u64,
    pub success_count: u64,
    pub last_used: DateTime,
    pub created_at: DateTime,
    pub invalidated_by: Option<Uuid>,  // 被哪条新写入触发失效
}
```

### Agent 反馈

```rust
pub struct AgentFeedback {
    pub query_hash: u64,
    pub agent_id: String,
    pub success: bool,
    pub feedback_note: Option<String>,
    pub timestamp: DateTime,
}
```

---

## 进化引擎类型（L2）

```rust
pub struct EvolutionEngineState {
    pub mode: EvolutionMode,
    pub anomaly_counter: u32,
    pub paradigm_shift_threshold: u32,
    pub ticks_since_last_shift: u32,
    pub shift_history: Vec<ShiftRecord>,
}

pub enum EvolutionMode { Incremental, ParadigmShift }

pub struct ShiftRecord {
    pub timestamp: DateTime,
    pub result: ShiftResult,
    pub old_framework_hash: String,
    pub new_framework_hash: Option<String>,
    pub improvement_pct: Option<f64>,
}

pub enum ShiftResult { Success, Rollback }

pub struct AnomalySignals {
    pub prediction_error_streak: u32,
    pub conflict_density_pct: f64,
    pub cache_hit_rate_trend: f64,  // 负值 = 下降
}
```

---

## 订阅管理类型（L7）

```rust
pub struct SubscriptionSource {
    pub id: Uuid,
    pub name: String,
    pub source_type: SubscriptionType,
    pub config: serde_json::Value,         // 源类型特定配置（URL/选择器/认证等）
    pub poll_interval: Duration,
    pub claimant_template: Claimant,
    pub base_confidence: f64,
    pub enabled: bool,
    pub last_polled: Option<DateTime>,
    pub error_count: u32,                  // 连续失败次数（超阈值自动禁用）
    pub tenant_id: String,
}

pub enum SubscriptionType { Rss, ApiPoll, WebScraping, SearchAlert }
```

---

## 联邦层类型（L8）

```rust
pub struct MetaKnowledgeEntry {
    pub instance_id: String,
    pub domain_tags: Vec<String>,
    pub expertise_score: f64,       // 该实例在该领域的专长度
    pub last_updated: DateTime,
}

pub struct FederationHealthCheck {
    pub diversity_entropy: f64,
    pub independence_score: f64,
    pub centralization_gini: f64,
    pub aggregation_vs_best: f64,  // > 0 表示聚合优于最佳单源
}
```

---

## PostgreSQL Schema

> pgvector 扩展需要预先启用：`CREATE EXTENSION IF NOT EXISTS vector;`

```sql
CREATE TABLE epistemic_claims (
    id UUID PRIMARY KEY,
    content TEXT NOT NULL,
    node_type TEXT NOT NULL,
    claimant JSONB NOT NULL,
    epistemic_status TEXT NOT NULL DEFAULT 'Asserted',
    confidence FLOAT NOT NULL DEFAULT 0.5,
    consolidation_stage TEXT NOT NULL DEFAULT 'FastTrack',
    durability FLOAT NOT NULL DEFAULT 1.0,
    -- 活跃度（S3 读即写）
    activation_weight FLOAT NOT NULL DEFAULT 0.5,
    access_count BIGINT NOT NULL DEFAULT 0,
    last_accessed TIMESTAMPTZ,
    -- 双时态
    t_valid_start TIMESTAMPTZ NOT NULL,
    t_valid_end TIMESTAMPTZ,
    t_known TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- 权限
    access_envelope JSONB NOT NULL,
    provenance JSONB NOT NULL,
    vector_id UUID,
    embedding vector,        -- pgvector 语义向量，维度运行时自动检测（1024 for BGE-M3, 3072 for text-embedding-3-large）
    last_prediction_error FLOAT,
    derived_from UUID[] DEFAULT '{}',
    needs_revalidation BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_claims_embedding ON epistemic_claims
  USING hnsw (embedding vector_cosine_ops);

CREATE TABLE conflict_records (
    id UUID PRIMARY KEY,
    claim_a_id UUID REFERENCES epistemic_claims(id),
    claim_b_id UUID REFERENCES epistemic_claims(id),
    conflict_type TEXT NOT NULL,
    detected_at TIMESTAMPTZ DEFAULT NOW(),
    resolution_status TEXT NOT NULL DEFAULT 'Open',
    resolution_note TEXT,
    elevated_insight_id UUID REFERENCES epistemic_claims(id)
);

-- 查询缓存（内存为主，PG 做持久化备份）
CREATE TABLE query_cache (
    query_hash BIGINT PRIMARY KEY,
    response JSONB NOT NULL,
    confidence FLOAT NOT NULL DEFAULT 0.6,
    hit_count BIGINT DEFAULT 0,
    success_count BIGINT DEFAULT 0,
    last_used TIMESTAMPTZ DEFAULT NOW(),
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Agent 反馈
CREATE TABLE agent_feedbacks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    query_hash BIGINT,
    agent_id TEXT NOT NULL,
    success BOOLEAN NOT NULL,
    feedback_note TEXT,
    timestamp TIMESTAMPTZ DEFAULT NOW()
);

-- 订阅源
CREATE TABLE subscription_sources (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    source_type TEXT NOT NULL,
    config JSONB NOT NULL,
    poll_interval INTERVAL NOT NULL DEFAULT '1 hour',
    claimant_template JSONB NOT NULL,
    base_confidence FLOAT NOT NULL DEFAULT 0.7,
    enabled BOOLEAN DEFAULT TRUE,
    last_polled TIMESTAMPTZ,
    error_count INTEGER DEFAULT 0,
    tenant_id TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- 进化引擎状态
CREATE TABLE evolution_state (
    id INTEGER PRIMARY KEY DEFAULT 1,
    mode TEXT NOT NULL DEFAULT 'Incremental',
    anomaly_counter INTEGER DEFAULT 0,
    ticks_since_last_shift INTEGER DEFAULT 0,
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE shift_history (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    timestamp TIMESTAMPTZ DEFAULT NOW(),
    result TEXT NOT NULL,
    old_framework_hash TEXT,
    new_framework_hash TEXT,
    improvement_pct FLOAT
);
```
