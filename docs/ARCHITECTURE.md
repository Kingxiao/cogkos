# CogKOS 架构设计

## 系统定位

CogKOS 是**纯后端认知管理系统**——它不运行 Agent，不做即时决策。

```
┌─ Agent 前端层 ──────────────────────┐       ┌─ CogKOS 后端 ──────────────────────────┐
│                                     │       │                                        │
│  Agent A ─┐                         │       │   L7: 摄入管道层                        │
│  Agent B ─┤── 决策引擎(Sys1/2)      │ MCP   │   L6: 查询层(MCP Server)               │
│  Agent C ─┘   操作记忆(本地)         │ ←───→ │   L5: 知识图谱层                        │
│               查询缓存(本地)         │       │   L4: 进化引擎层                        │
│                                     │       │   L3: 异步整合层(Sleep-time)            │
│  Agent 负责：即时决策、操作记忆       │       │   L2: 外部知识层                        │
│  Agent 不负责：长期记忆、知识进化     │       │   L1: 持久化层                          │
└─────────────────────────────────────┘       │                                        │
                                              │  CogKOS 负责：长期知识、进化、预测       │
                                              └────────────────────────────────────────┘
```

---

## 后端架构（CogKOS 七层）

### L1: 持久化层

| 存储 | 存什么 |
|------|--------|
| **PostgreSQL** | 元数据 / AccessEnvelope / 审计日志 / GDPR |
| **PostgreSQL pgvector** | 语义向量（替代原 Qdrant，使用 `<=>` 余弦距离算子） |
| **FalkorDB** | EpistemicClaim 节点 / 关系 / ConflictRecord |
| **S3** | 原始文件 / 快照 |

多租户：数据库级别隔离（每个客户实例独立 DB）。

> **向量检索说明**：语义向量存储和检索统一使用 PostgreSQL pgvector 扩展，不再依赖独立的向量数据库。pgvector 同时支持语义检索和语义距离计算（新颖度评估）。

---

### L2: 外部知识与订阅管理层

详见 [EXTERNAL_KNOWLEDGE_DESIGN.md](./EXTERNAL_KNOWLEDGE_DESIGN.md)。

#### 订阅管理模块（用户可配置）

```
┌─ 订阅管理 ────────────────────────────────────────────┐
│                                                        │
│  订阅源（管理员可增删改）：                               │
│  ├── RSS Feed（行业资讯、竞品博客、学术预印本）            │
│  ├── API 轮询（财务数据、政策公报、市场指数）              │
│  ├── Web Scraping 计划（竞品官网、招标平台）              │
│  └── 搜索订阅（定期关键词搜索，类似 Google Alerts）       │
│                                                        │
│  调度器：                                               │
│  ├── 每源独立轮询频率（5分钟 ~ 每日）                     │
│  ├── 去重（URL/内容 hash 级别）                          │
│  └── 限流（防封禁）                                     │
│                                                        │
│  输出 → 全部经过 L7 摄入管道 → EpistemicClaim            │
└────────────────────────────────────────────────────────┘
```

#### 文档自动分类（用户无需手动分目录）

```
文件上传（任意格式/任意目录结构/批量扔进来）
  ↓
Phase 1 粗分类（无 LLM，秒级）：
  ├── 文件名解析："XX公司2025年报.pdf" → Entity=XX公司, Type=年报
  ├── 格式推断 → 初始 NodeType
  └── → File 类型 EpistemicClaim + 粗分类标签
  ↓
Phase 3 深度分类（LLM 提取）：
  ├── 行业识别（制造/零售/金融/...）
  ├── 实体提取（公司/产品/人物）
  ├── 文档类型（报告/方法论/案例/论文/年报）
  ├── 关键结论/预测/数据点 → EpistemicClaim
  └── 自动建立图谱关系：
      [File: XX年报] --ABOUT--> [Entity: XX公司]
      [File: XX年报] --INDUSTRY--> [Entity: 制造业]
      [File: XX年报] --CONTAINS_PREDICTION--> [Prediction: 营收+12%]
```

**物理存储位置不重要——知识图谱的关系才是"位置"。**

---

### L3: 异步整合层（Sleep-time Compute）

定时或事件驱动的后台任务，受计算预算约束（E9）。

| 任务 | 预算上限 | 频率 | 触发 |
|------|---------|------|------|
| `detect_conflicts` | < 5% | 每次写入 | 事件驱动 |
| `consolidate` | < 10% | 周期批处理 | 定时 |
| `decay` | < 5% | 日级 | 定时 |
| `validate_predictions` | < 5% | 结果回流时 | 事件驱动 |
| `elevate_insights` | ≤ 30% | 冲突密度超阈值 | 事件驱动 |
| `health_check` | < 5% | 日级 | 定时 |

#### 贝叶斯聚合

```rust
fn bayesian_aggregate(claims: &[EpistemicClaim]) -> f64 {
    let prior_log_odds = 0.0;
    let total = claims.iter()
        .map(|c| log_odds(c.confidence))
        .sum::<f64>() + prior_log_odds;
    odds_to_prob(total)
}

// 独立来源去重
// Phase 1: source_id 去重（够用）
// Phase 3+: provenance chain 增强
fn count_independent_sources(claims: &[EpistemicClaim]) -> usize {
    claims.iter()
        .map(|c| &c.provenance.source_id)
        .collect::<HashSet<_>>()
        .len()
}
```

---

### L4: 进化引擎层

双模式进化——不是某个"智能模块"的功能，是系统的结构属性。

```
                        anomaly_counter > threshold
  [渐进模式] ──────────────────────────────────→ [范式转换模式]
       ↑                                              │
       │   A/B 测试完成 or 回退                        │
       └──────────────────────────────────────────────┘
```

#### 渐进模式（99% 时间）

| 操作 | 做什么 | 对应进化要素 |
|------|--------|-------------|
| 贝叶斯聚合 | 多条 Assertion → Belief | 遗传 |
| 知识衰减 | 未确认知识置信度下降 | 选择（淘汰） |
| 预测验证 | 预测 vs 实际 → 回写误差 | 选择 |
| 冲突检测 | 标记矛盾 Claim 对 | 变异 |

#### 范式转换模式（反常触发时）

**反常信号检测**：
- 信号 1：预测误差持续 > 阈值（连续 N 个验证周期）
- 信号 2：特定领域冲突密度异常升高
- 信号 3：MCP 查询缓存命中率持续下降（知识与需求脱节）

**转换流程**：
1. 快照当前状态，暂停渐进进化
2. LLM 沙箱生成候选新解释框架
3. A/B 测试：新旧框架各自预测，对比准确率
4. 新框架 > 旧框架 10% → 原子切换。否则回退
5. 重置反常计数器

**计算预算**：范式转换最多占 50% 计算资源，分阶段执行。

---

### L5: 知识图谱层

EpistemicClaim 在 FalkorDB 中的组织：

```
[Claim: Entity] ──RELATES_TO──→ [Claim: Entity]
[Claim: Belief] ──DERIVED_FROM──→ [Claim: Assertion]
[Claim: A] ──IN_CONFLICT──→ [ConflictRecord] ←──IN_CONFLICT── [Claim: B]
```

按 `consolidation_stage` 分层：
- **FastTrack**：原始断言，只增不改
- **Consolidated**：系统聚合的当前最佳理解
- **Insight**：跨来源/跨领域模式发现

---

### L6: 查询层（MCP Server）

CogKOS 对外的核心接口。**查询缓存 + 完整推理的双路径设计**。

**MCP 实现**：基于 rmcp SDK（MCP 标准协议），使用 `#[tool]`、`#[tool_router]`、`#[tool_handler]` 宏定义工具。传输层支持 stdio + Streamable HTTP（通过 `StreamableHttpService`）。工具参数使用 `Parameters<T>` 配合 `schemars::JsonSchema` 自动生成 JSON Schema。

```
Agent MCP请求 → 查询缓存查找（System 1）
  ├── 命中且置信度高 → 直接返回（更新缓存命中统计）
  └── 未命中或置信度低 → 完整推理（System 2）
        → 权限过滤（AccessEnvelope）
        → 语义检索（pgvector）
        → 图上激活扩散（FalkorDB）
        → 结果合并排序
        → 冲突检测
        → LLM 轻量预测
        → 知识空洞检测
        → 结构化响应
        → 写入查询缓存
        → 更新命中知识的 activation_weight  ← 读即写（S3）
```

#### 图上激活扩散

向量检索找到初始节点后，沿图边传播激活：

```
向量命中节点 A (activation=1.0)
  → A --CAUSES--> B (weight=0.8) → B.activation += 1.0 × 0.8 × decay
  → B --SIMILAR_TO--> C (weight=0.6) → C.activation += ...
  → 收集 activation > threshold 的全部节点，与向量结果合并
```

**价值**：发现向量相似度找不到的**间接关联**（A 和 C 在向量空间不相似，但通过因果链连接）。

#### MCP 响应格式

```json
{
  "query_context": "竞品X对中小企业的适用性",
  "best_belief": {
    "content": "竞品X中小企业满意度低于行业均值",
    "confidence": 0.78,
    "based_on": 5,
    "consolidation_stage": "Consolidated"
  },
  "related_by_graph": [
    { "content": "竞品X的售后响应慢于同类", "relation": "CAUSES", "activation": 0.72 }
  ],
  "conflicts": [...],
  "prediction": {
    "content": "推荐竞品X给中小企业的风险较高",
    "confidence": 0.72,
    "method": "llm_belief_context"
  },
  "knowledge_gaps": ["缺少竞品X 2026版本数据"],
  "freshness": { "newest_source": "2026-02-10", "staleness_warning": false },
  "cache_status": "miss"
}
```

#### 查询缓存设计

```rust
struct QueryCacheEntry {
    query_hash: u64,          // 查询内容哈希
    response: McpQueryResponse,
    confidence: f64,          // 缓存置信度
    hit_count: u64,
    success_count: u64,       // Agent 反馈决策成功的次数
    last_used: DateTime,
    created_at: DateTime,
}

// 缓存失效条件（任一触发则跳过缓存）：
// 1. confidence < 0.5
// 2. 缓存创建后相关知识有写入更新
// 3. success_rate < 0.4
// 4. 缓存超过 TTL（默认 1 小时）
```

#### Agent 反馈回路

```
Agent 使用 CogKOS 的预测做决策 → 决策结果发生
  → Agent 回传 feedback { query_hash, success: bool }
  → CogKOS 更新：
    1. 查询缓存的 success_count（即时）
    2. 相关知识的 prediction_error（Sleep-time）
    3. 进化引擎的反常信号检测（累计）
```

---

### L7: 摄入管道层

```
外部输入 → 格式检测/解析（多格式支持）
         → Claimant/来源标注
         → 权限推断（AccessEnvelope）
         → FastTrack 写入（L5 + L1）
         → 向量化（pgvector）
         → 语义距离计算
         →距离 > 阈值 → "新信息" → 触发 Sleep-time 聚合
         → 距离 < 阈值 → "确认" → 增强已有知识置信度
         → 冲突检测 → 矛盾则创建 ConflictRecord
```

---

### 联邦层（FROZEN）

> **STATUS: FROZEN** — 联邦层在 V1 中不激活。代码保留供 V2/V3 使用。

多实例间 Insight 共享 + 群体智慧四条件量化健康检查。

#### 四条件量化检查（来自 collective-intelligence skill）

| 条件 | 检测指标 | 健康范围 | 告警 |
|------|---------|---------|------|
| **多样性** | Insight 来源分布的香农熵 | > log2(K)×0.7 | 低于则知识来源过于单一 |
| **独立性** | 同上——来源的 provenance 独立性 | — | 多个来源溯源到同一上游 |
| **去中心化** | Insight 影响力基尼系数 | < 0.3 | 少数 Insight 主导预测 |
| **聚合有效性** | 聚合预测 vs 最佳单源预测 | 聚合 > 单源 | 聚合不如最佳单源则换聚合方法 |

#### 事务性记忆（来自 collective-intelligence skill）

```
元知识目录：
  实例A（制造业） → 擅长：供应链、质量管理、MES
  实例B（零售）   → 擅长：消费者分析、SKU管理、O2O
  实例C（内部）   → 擅长：竞品分析、行业趋势

跨实例查询路由：
  查询"供应链优化" → 路由到实例A
  查询"消费者画像" → 路由到实例B
```

---

## 前端与后端的交互协议

### MCP 工具定义（CogKOS 暴露给 Agent）

| 工具 | 方向 | 说明 |
|------|------|------|
| `query_knowledge` | Agent → CogKOS | 带上下文的结构化查询，返回 McpQueryResponse |
| `submit_experience` | Agent → CogKOS | Agent 推送经验/观察，作为 Assertion 写入 |
| `submit_feedback` | Agent → CogKOS | 对之前查询结果的成功/失败反馈 |
| `report_gap` | Agent → CogKOS | Agent 主动报告发现的知识空洞 |
| `get_meta_directory` | Agent → CogKOS | 查询元知识目录（谁擅长什么） |

### MCP Sampling（CogKOS → Agent，MCP 2026 新增）

MCP 2026 规范支持 **Server → Host LLM** 的反向请求（Sampling）。CogKOS 可利用此能力：

| 场景 | 做什么 |
|------|--------|
| 冲突分析委托 | CogKOS 发现冲突后，通过 Sampling 请求 Agent 的 LLM 分析冲突原因 |
| 知识验证请求 | 知识衰减到阈值时，请求 Agent 确认是否仍有效 |
| 预测生成委托 | Sleep-time 需要预测时，借用 Agent 的 LLM 能力 |

> 价值：CogKOS 不必内置独立 LLM——可以借用接入 Agent 的 LLM 做计算。降低后端成本。

### Agent 不应做的事

| 禁止操作 | 原因 |
|---------|------|
| 直接修改 EpistemicClaim 的置信度 | 置信度由进化引擎管理 |
| 绕过 AccessEnvelope 查询 | 权限是硬约束 |
| 在 Agent 本地做知识聚合 | 聚合是后端的职责 |
| 把 CogKOS 当临时文件中转站 | CogKOS 管理业务文档（S3）+ 提取知识——但不存 Agent 运行时临时数据 |

---

## 进化闭环

```
            ┌──── L6 查询 → Agent 决策 ────┐
            │                               │
            │                               ↓
    L5 图谱 ←── L3 整合 ←── L7 写入 ←── Agent 经验回流 + 反馈
            │                               │
            │                               │
            ↓                               │
    L4 进化引擎 ←─── 反常信号累积 ←─────────┘
         │
         ├── 渐进模式: 聚合/衰减/验证
         └── 范式转换: LLM沙箱 → A/B → 切换/回退
```

---

## 原则覆盖验证

| 原则 | L1 | L2 | L3 | L4 | L5 | L6 | L7 |
|------|----|----|----|----|----|----|---|
| S1 预测 | | | | | | ✅ | |
| S2 快/慢 | | | ✅ | | ✅ | | ✅ |
| S3 读=写 | | | | | | ✅ | |
| S4 保质期 | | | ✅ | | ✅ | | |
| S5 进化 | | | ✅ | ✅ | ✅ | | ✅ |
| S6 双路径 | | | | | | ✅ | |
| E1 断言 | | | | | ✅ | | ✅ |
| E2 冲突 | | | ✅ | ✅ | ✅ | ✅ | |
| E3 权限 | ✅ | | | | ✅ | ✅ | ✅ |
| E7 MCP | | | | | | ✅ | |
| E8 分离 | | | | | | ✅ | |
| E9 预算 | | | ✅ | ✅ | | | |

---

## 规模化路径：从 10K 到 10M Agent

### 流量模型

```
Agent 数量    瞬时并发(30%)   平均 QPS        峰值 QPS        单日新增 Claim
──────────────────────────────────────────────────────────────────────────
10 万         3 万           5K              15K             100 万
100 万        30 万          50K             150K            1000 万
1000 万       300 万         500K            1.5M            1 亿
```

### 当前架构天花板（V1 单实例）

| 组件 | 当前选型 | 单实例上限 | 瓶颈原因 |
|------|---------|-----------|---------|
| MCP Gateway | Rust (tokio) | ~500K 并发连接 | OK — 无状态水平扩展 |
| PostgreSQL | 单实例 | ~50K QPS 读 | 不支持水平分片 |
| FalkorDB | 单实例内存图 | ~20K QPS 图遍历 | 支持 Redis Cluster 分片，但需集群化 |
| PostgreSQL pgvector | 单实例 | ~5K QPS 向量检索 | 需调优索引（HNSW/IVFFlat） |
| 读即写 | 每次查询写 activation_weight | ~10K 写/秒 | 大规模不可能每次都写 |

---

### V1: 10 万 Agent（当前设计，无需改动）

```
Agent → MCP Gateway (Rust, 单实例) → 存储 (各单实例)
```

| 组件 | 选型 | 部署 |
|------|------|------|
| MCP Gateway | Rust + tokio | 单实例（8C16G） |
| 关系库 + 向量 | PostgreSQL + pgvector | 单实例 + 1 读副本 |
| 图数据库 | FalkorDB | 单实例（内存 32G） |
| 对象存储 | S3 | — |

**读即写**：100% 采样，每次查询更新 activation_weight。

**激活扩散**：查询时实时计算。

**一致性**：强一致性 (ACID)。

**估算成本**：~¥5K/月（3-5 台服务器）。

---

### V2: 100 万 Agent

```
Agent → L4 负载均衡
          │
          ▼
   Gateway Cluster (Rust × 10-20 台)
          │
          ├── 命中 → Dragonfly 缓存集群 (3 节点)
          │
          └── 未命中 → 存储层 (读写分离)
                        ├── TiDB (3 节点)
                        ├── FalkorDB Cluster (分片)
                        └── pgvector (TiDB 内或独立 PG 集群)
```

#### V1 → V2 变更清单

| 变更 | 做什么 | 为什么 |
|------|--------|--------|
| **+分布式缓存** | 引入 Dragonfly（3 节点） | 80% 命中 → 后端降压 5 倍 |
| **关系库换型** | PostgreSQL → **TiDB**（3 节点） | 分布式 SQL，水平扩展 |
| **Gateway 多实例** | 10-20 台 + L4 负载均衡 | 30 万并发连接 |
| **pgvector 扩展** | 独立 PG 集群 + pgvector 或迁移至分布式向量方案 | 向量检索分散压力 |
| **+消息队列** | 引入 NATS JetStream（3 节点） | 写入全部异步化 |
| **读即写降级** | 采样率 10% + 批量刷盘 | 写 QPS 从 50K 降到 5K |

#### V2 缓存策略

| 缓存内容 | Key 格式 | TTL | 失效策略 |
|---------|---------|-----|---------|
| 查询结果 | `t:{tid}:q:{hash}` | 10-60 分钟 | 对应知识写入时主动失效 |
| 热门知识 | `t:{tid}:c:{id}` | 5 分钟 | 写入时失效 |
| 激活扩散结果 | `t:{tid}:s:{node}` | 30 分钟 | Sleep-time 定期刷新 |

#### V2 写入管道（异步化）

```
Agent 写入请求 → Gateway（只做校验，不写库）
    → NATS JetStream
    → Workers 异步消费：
        ├── experience.{tid}  → 写入 EpistemicClaim
        ├── feedback.{tid}    → 更新缓存 + prediction_error
        └── activation.{tid}  → 批量更新 activation_weight
```

**估算成本**：~¥50K/月（~30 台服务器）。

---

### V3: 1000 万 Agent

```
10M Agent
    │
    ▼
DNS 智能解析 (多区域)
    │
    ├── 华东 ──┐
    ├── 华北 ──┤── 各区域独立接入
    └── 海外 ──┘
                │
                ▼
         L4 负载均衡
                │
                ▼
┌─ Edge Cache (Dragonfly Cluster, 5 节点) ────────────┐
│  预计算查询结果 + 激活扩散结果                          │
│  目标命中率: > 80%  → 穿透到后端: < 300K QPS          │
└─────────────────────────────────────────────────────┘
         │ 缓存未命中
         ▼
┌─ MCP Gateway Cluster (Rust + tokio + io_uring) ─────┐
│  60 台，无状态                                        │
└─────────────────────────────────────────────────────┘
         │
         ├── 读路径 → 分片路由器 → 按 tenant_id 分片的存储集群
         │                          ├── TiDB 分片 × 10
         │                          ├── Neo4j 分片 × 10
         │                          └── pgvector 分片 × 10
         │
         └── 写路径 → NATS JetStream → 异步 Workers (20 台)
```

#### V2 → V3 变更清单

| 变更 | 做什么 | 为什么 |
|------|--------|--------|
| **图数据库评估** | FalkorDB Cluster 或 Neo4j Infinigraph | 按数据量和查询模式选择（见下表） |
| **向量库评估** | pgvector 集群 或 Milvus | Milvus 高吞吐场景；pgvector 低运维成本 |
| **租户分片** | 按 tenant_id 分 10 个分片 | 每分片独立的完整存储栈 |
| **多区域部署** | DNS 智能解析 + 区域接入 | 全球 Agent 就近接入 |
| **时序数据** | PostgreSQL 表（预测误差历史等） | 统一存储栈，减少组件数 |
| **读即写降级** | 采样率 5% + 批量异步写入 | 5% 采样误差 < 3% |
| **激活扩散预计算** | 实时 → Sleep-time 预计算 + 缓存 | 实时 O(V+E) 在 1M QPS 下不可行 |

**V3 图数据库选型依据**：

| | FalkorDB Cluster | Neo4j Infinigraph |
|---|---|---|
| 分片方式 | Redis slot-based | 属性分片（图结构不拆分） |
| 多租户 | 原生多图隔离 | 需手动管理 |
| 延迟 | 亚毫秒（内存图） | 毫秒级 |
| 数据量上限 | 受内存限制 | 100TB+（Infinigraph） |
| 适用 | 高频低延迟、中等数据量 | 超大数据量、复杂分析 |
| **默认选择** | **优先** | 数据量 > 内存时切换 |

#### V3 分片策略

```
分片路由器
  │
  ├── tenant 001-100    → Shard A (TiDB×3 + Neo4j×3 + pgvector×2)
  ├── tenant 101-200    → Shard B
  ├── ...
  └── tenant 901-1000   → Shard J

大客户 (>10万 Agent) → 独占分片
小客户 → 合并（每分片 ~100 租户）
```

**为什么按 tenant 分片**：
- 95% 查询在单租户内完成 → 无需跨片
- 联邦查询（跨租户）本来就是异步的
- 天然满足数据隔离合规

#### V3 读即写的采样方案

```rust
fn should_update_activation(agent_count: u64) -> bool {
    let sample_rate = match agent_count {
        0..=100_000         => 1.0,    // V1: 100%
        100_001..=1_000_000 => 0.1,    // V2: 10%
        _                   => 0.05,   // V3: 5%
    };
    rand::random::<f64>() < sample_rate
}

// 批量写入 buffer
struct ActivationBatch {
    updates: Vec<(Uuid, f64)>,
    flush_interval: Duration,  // 每 5 秒或满 1000 条时刷盘
}
```

**数学保证**：5% 采样率 × 1000 次查询 = ~50 次更新。大数定律保证相对误差 < 3%。

#### V3 激活扩散预计算

```
Sleep-time Worker（每 30 分钟）：
  1. 取 Top 1000 高频查询入口节点
  2. 预跑激活扩散
  3. 结果写入 Dragonfly 缓存

查询时：
  缓存命中 → 直接返回关联知识
  缓存未命中 → 退化为纯向量检索（不做实时扩散）
```

#### V3 技术栈总览

| 层 | 选型 | 语言 | 选择理由 |
|---|---|---|---|
| MCP Gateway | **Rust (tokio + io_uring)** | Rust | 零 GC、2KB/连接 |
| 分布式缓存 | **Dragonfly** | — | 单实例百万 QPS，Redis 兼容 |
| 消息队列 | **NATS JetStream** | — | 轻量、Rust 原生客户端 |
| 知识图谱 | **FalkorDB Cluster** 或 Neo4j Infinigraph | — | 默认 FalkorDB（低延迟+多租户），数据超内存则 Neo4j |
| 向量检索 | **pgvector** 或 **Milvus** | — | pgvector 统一存储栈；高吞吐场景考虑 Milvus |
| 关系库 | **TiDB** | — | 分布式 SQL |
| 时序数据 | **PostgreSQL** | — | 统一存储栈，预测误差历史等使用 PG 表 |
| Sleep-time | **Rust** 或 **Go** | Rust/Go | Go 开发效率高 |
| 订阅调度 | **Go** | Go | HTTP 抓取+定时生态完善 |
| 对象存储 | **S3** | — | 已无限扩展 |

#### V3 成本估算

| 组件 | 规格 | 数量 | 月成本 |
|------|------|------|--------|
| Gateway (Rust) | 8C16G | 60 台 | ~¥45K |
| Dragonfly | 16C64G | 5 台 | ~¥15K |
| NATS | 8C16G | 3 台 | ~¥7K |
| TiDB (10 分片 × 3) | 16C64G | 30 台 | ~¥90K |
| Neo4j (10 分片 × 3) | 16C128G | 30 台 | ~¥150K |
| pgvector (10 分片 × 2) | 16C64G | 20 台 | ~¥60K |
| Workers | 8C16G | 20 台 | ~¥15K |
| S3 (100 TB) | — | — | ~¥15K |
| **总计** | | **~169 台** | **~¥397K/月** |

**单 Agent 成本**：¥397K / 10M = **~¥0.04/Agent/月（4 分钱）**。

---

### 升级触发条件

不提前过度设计。当以下指标触发时执行对应版本升级：

| 指标 | V1 → V2 触发 | V2 → V3 触发 |
|------|-------------|-------------|
| 并发连接 | > 50K | > 500K |
| DB QPS | PG > 30K | TiDB 单分片 > 40K |
| 图遍历延迟 P99 | > 50ms | > 50ms（分片后仍超） |
| 向量检索 QPS | > 3K | > 30K（集群后仍超） |
| 缓存命中率 | — | < 60% |
| 写入延迟 P99 | > 100ms | > 200ms |
| Agent 总数 | > 10 万 | > 100 万 |

> **核心判断**：Rust 作为核心不变。10M 规模下语言只占 20% 影响——80% 取决于数据架构（分片/缓存/CQRS/异步化）。先做对 V1，规模化是渐进升级。
