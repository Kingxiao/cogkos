# 外部知识与文档管理设计

## 定位

CogKOS 管理全部业务文档的全生命周期：**存储 → 解析 → 知识提取 → 追溯**。

这是一个闭环——原始文档是知识的来源，知识的 ProvenanceRecord 指回原始文档，文档更新时知识自动标记需要重新验证。

```
文档存储(S3)  ←── 追溯来源 ──── 知识图谱(FalkorDB)
     │                              ↑
     ↓                              │
   解析(L9) ─── 知识提取 ──→ EpistemicClaim 写入
     │                              ↑
     ↓                              │
   文档更新 ─── 标记 ──→ needs_revalidation
```

---

## 文档管理（CogKOS 核心职责）

### 支持格式

| 格式 | 解析难度 | 技术方案 |
|------|---------|---------|
| **Markdown/TXT** | ★☆ | 直接读取 |
| **PDF（文字版）** | ★★ | pdfplumber / marker |
| **PDF（扫描版）** | ★★★ | OCR (Surya/Tesseract) + 版面分析 |
| **Word (.docx)** | ★★ | python-docx / LibreOffice |
| **PPT (.pptx)** | ★★★ | python-pptx + 图表视觉理解 |
| **Excel** | ★★ | openpyxl + Schema 推断 |
| **音频** | ★★★ | Whisper STT → 文本 |
| **视频** | ★★★★ | Whisper (音轨) + 关键帧提取 |

### 文档存储结构

```
S3 Bucket (per tenant)
├── /raw/           ← 原始文件（不修改）
├── /parsed/        ← 解析后的结构化文本
└── /snapshots/     ← 知识图谱快照
```

每个文件在 FalkorDB 中有对应的 `EpistemicClaim { node_type: File }`，包含文件路径、格式、解析状态。

### 三层递进初始化（新客户部署）

| 层 | 时间 | 做什么 | Agent 可用程度 |
|----|------|--------|---------------|
| **L1: 元数据索引** | T+0, 小时级 | 全部文件 → 文件名/目录/类型 → File Claim | "有什么文档"可查 |
| **L2: 全文语义索引** | T+1天-1周 | 解析为文本 → 分块 → Qdrant 向量化 | 语义检索可用 |
| **L3: 知识提取** | T+1周起 | LLM 提取实体/关系/预测 → EpistemicClaim | 预测和冲突检测可用 |

**关键**：Agent 在 L2 完成后就能工作——语义检索 + LLM 实时推理已覆盖大部分需求。L3 是增量提升。

---

## 订阅管理（L7 子模块）

用户/管理员可配置的外部信息订阅系统。所有订阅内容统一进入 L9 摄入管道。

### 订阅源类型

| 类型 | 配置项 | 示例 |
|------|-------|------|
| **RSS Feed** | URL + 轮询频率 | Gartner 博客、arXiv 预印本、竞品更新日志 |
| **API 轮询** | API endpoint + 认证 + 频率 | 财务数据 API、政策公报 API |
| **Web Scraping** | URL + CSS 选择器 + 频率 | 竞品官网、招标平台、招聘动态 |
| **搜索订阅** | 关键词 + 搜索引擎 + 频率 | "XX行业 数字化转型"（类似 Google Alerts） |

### 调度与去重

```rust
pub struct SubscriptionSource {
    pub id: Uuid,
    pub name: String,
    pub source_type: SubscriptionType,   // RSS / API / Scraping / Search
    pub config: serde_json::Value,       // 源类型特定配置
    pub poll_interval: Duration,         // 轮询间隔（5min ~ 24h）
    pub claimant_template: Claimant,     // 该源产出内容的默认 Claimant
    pub base_confidence: f64,            // 该源的默认基础置信度
    pub enabled: bool,
    pub last_polled: Option<DateTime>,
    pub tenant_id: String,
}

pub enum SubscriptionType { Rss, ApiPoll, WebScraping, SearchAlert }
```

**去重策略**：
- URL 级别：同一 URL 不重复摄入
- 内容 hash 级别：不同 URL 但内容相同也去重
- 语义级别：Phase 3+ 用向量距离判断是否"实质相同"

**限流**：每源独立限流，防止被来源服务器封禁。

---

## 文档自动分类

用户不需要手动分目录——批量扔进来即可，CogKOS 自动组织。

### 两阶段分类

| 阶段 | 依赖 | 时间 | 做什么 |
|------|------|------|--------|
| **粗分类** | 无 LLM | 秒级 | 文件名解析 + 格式推断 + 元数据标签 |
| **深度分类** | LLM | 分钟-小时级 | 行业/公司/类型/主题识别 + 实体/关系/预测提取 |

### 粗分类规则（Phase 1 即可用）

```
"XX公司2025年度报告.pdf"
  → Entity 候选: "XX公司"（中文公司名模式匹配）
  → Type 推断: "年度报告"（关键词匹配）
  → Year: 2025（数字提取）
  → File EpistemicClaim { node_type: File, tags: ["年报", "XX公司", "2025"] }

"数字化转型方法论V3.docx"
  → Type 推断: "方法论"
  → Topic 候选: "数字化转型"
  → File EpistemicClaim { node_type: File, tags: ["方法论", "数字化转型"] }
```

### 深度分类（Phase 3 LLM 提取）

LLM 读取文档内容后提取：

| 提取内容 | 映射到 | 图谱关系 |
|---------|--------|---------|
| 所属行业 | Entity Claim | `[File] --INDUSTRY--> [Entity: 行业]` |
| 涉及公司 | Entity Claim | `[File] --ABOUT--> [Entity: 公司]` |
| 文档类型 | File 标签 | — |
| 关键结论 | Belief Claim | `[File] --CONTAINS--> [Belief]` |
| 预测数据 | Prediction Claim | `[File] --CONTAINS_PREDICTION--> [Prediction]` |
| 数据点 | Assertion Claim | `[File] --CONTAINS_DATA--> [Assertion]` |
| 方法论要点 | Insight Claim | `[File] --DESCRIBES_METHOD--> [Insight]` |

**Agent 查询示例**：
```
Agent: "制造业有哪些可用资料？"
→ 图谱遍历: MATCH (f:File)-[:INDUSTRY]->(e:Entity {content: '制造业'}) RETURN f
→ 返回全部制造业相关文件 + 从中提取的知识
```

---

## 外部知识获取

### 来源与置信度

| 来源 | 进入方式 | Claimant Type | 基础置信度 |
|------|---------|--------------|-----------|
| 行业报告 (Gartner) | 文件上传+PDF解析 | ExternalPublic | 0.85-0.95 |
| 学术论文 | RSS/API | ExternalPublic | 0.90-0.95 |
| 新闻/推特 | RSS | ExternalPublic | 0.50-0.70 |
| 财务数据 | API→ClickHouse | System | 0.95 |
| 竞品公开信息 | Web Scraping | ExternalPublic | 0.60-0.75 |

### 处理管道

```
订阅源/文件上传/API → S3存储 → 解析(OCR+NLP) → 自动分类 → 知识提取 → EpistemicClaim → L9写入
                                                                          ↓
                                                          ProvenanceRecord → 指回 S3 原始文件
```

---

## 文档-知识闭环

### 文档更新检测

当同一来源的文档被更新时：
1. 新版文件存入 S3 `/raw/`
2. 重新解析 + 知识提取
3. 与旧版知识做语义距离比较
4. 新增/变化的知识写入，旧知识标记 `needs_revalidation`
5. 矛盾处理：旧版知识 vs 新版知识 → ConflictRecord { conflict_type: TemporalShift }

### 知识到文档的反向追溯

Agent 查询到某条知识时，可通过 ProvenanceRecord 追溯到：
- 原始文件路径（S3）
- 文件中的具体位置（页码/段落/时间戳）
- 文件被解析的时间
- 文件是否已有更新版本

---

## 冲突检测（外部 vs 内部）

```
外部报告预测："市场增长35%"
内部分析预测："市场增长22%"
→ 创建 ConflictRecord { conflict_type: SourceDisagreement }
→ Sleep-time 分析冲突原因
```

---

## 健康监测

| 指标 | 阈值 | 动作 |
|------|------|------|
| 来源更新频率 | >90天无更新 | 标记 needs_revalidation |
| 预测准确率 | <40% | 降低来源基础置信度 |
| 覆盖率 | 关键领域无外部数据 | 生成 knowledge_gap 警告 |
| **文档解析覆盖率** | 未解析文件 > 10% | 触发补充解析任务 |

---

## 实施阶段

| Phase | 文档管理能力 | 外部知识能力 |
|-------|-------------|-------------|
| Phase 1 | PDF/Word/Markdown 解析 + S3 存储 + 三层初始化 | 手动文件上传 |
| Phase 2 | 文档更新检测 + needs_revalidation 标记 | — |
| Phase 3 | LLM 深度知识提取 + 知识-文档反向追溯 | RSS 自动摄入 + LLM 提取 |
| Phase 4 | 全格式（含音视频） | API 集成（财务/竞品） |
| Phase 5 | 自动质量评级 | 全自动管道 |
