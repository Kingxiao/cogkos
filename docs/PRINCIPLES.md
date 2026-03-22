# CogKOS 设计基础

## 系统定位

> **CogKOS 是 Agent 生态的共享大脑——纯后端认知管理系统。**
>
> 它不运行 Agent，不做即时决策——它存储、进化和提供知识。
> Agent 产品是独立的前端，通过 MCP 接口与 CogKOS 交互。

---

## 前后端分离：第一性原理推导

### 为什么必须分离

生物学类比不是装饰——它直接推导出正确的模块边界：

| | 个体生物（Agent 前端） | 种群基因库（CogKOS 后端） |
|---|---|---|
| **时间尺度** | 毫秒-分钟（即时决策） | 小时-年（知识进化） |
| **核心能力** | 感知→决策→行动（C1-C4） | 记忆→进化→预测（S1-S5）|
| **存储** | 操作记忆（短期、一次性） | 长期知识（持久、可进化） |
| **失败代价** | 单次决策失败 | 整个知识体系退化 |
| **更新频率** | 每次请求 | 异步批处理 |

**推论**：把两者放在同一个系统里 = 把心脏和图书馆放在同一个建筑里。它们的运行节奏、失败模式、扩容策略完全不同。

### 边界定义

```
┌──────────────────────────┐         ┌──────────────────────────────────┐
│  Agent 前端（产品层）      │         │  CogKOS 后端（认知管理层）         │
│                          │         │                                  │
│  ┌─────────────────────┐ │   MCP   │  ┌────────────────────────────┐  │
│  │ 操作记忆(短期)       │ │ ←────→  │  │ 长期知识图谱(进化)          │  │
│  │ 决策引擎(Sys1/Sys2) │ │         │  │ Sleep-time 整合            │  │
│  │ 查询结果缓存         │ │         │  │ 冲突检测/提炼              │  │
│  │ Agent 自身状态       │ │         │  │ 预测生成                   │  │
│  └─────────────────────┘ │         │  │ 联邦化共享                  │  │
│                          │         │  └────────────────────────────┘  │
│  Agent A (Forge)         │         │                                  │
│  Agent B (客户Bot)       │         │  独立部署、独立扩容                │
│  Agent C (内部顾问)      │         │  可多实例（内部+客户）             │
└──────────────────────────┘         └──────────────────────────────────┘
```

### 前端负责什么 / 后端负责什么

| 职责 | 前端（Agent） | 后端（CogKOS） |
|------|-------------|---------------|
| 即时决策 | ✅ Agent 自己做 | ❌ 不做 |
| 操作记忆 | ✅ Agent 本地 | ❌ 不存 |
| 查询缓存（System 1） | ✅ Agent 本地缓存 | ❌ |
| **文档存储与管理** | ❌ | ✅ S3 + 解析 + 知识提取 |
| 长期知识存储 | ❌ | ✅ |
| 知识进化 | ❌ | ✅ |
| 冲突检测/解决 | ❌ | ✅ |
| 预测生成 | ❌ | ✅ |
| 经验回流（写入） | ✅ 推送经验 | ✅ 接收并存储 |
| 权限管理 | ❌ | ✅ |
| 联邦共享 | ❌ | ✅ |

### 能力边界：CogKOS 做什么 / Agent 做什么

**场景**：用户问"根据已存储的 XX 公司资料，其供应链能力怎么样？"

```
CogKOS 返回（原材料）：
  ✅ 检索：所有关于 XX 公司供应链的 EpistemicClaim（含置信度）
  ✅ 聚合：多来源贝叶斯聚合 → Consolidated Belief
  ✅ 冲突：年报说稳定 vs 新闻报道过断供 → ConflictRecord
  ✅ 关联：激活扩散发现 XX 供应商最近也出问题
  ✅ 空洞："缺少 XX 公司 2026 年供应链数据"
  ✅ 轻量预测："供应链风险中等偏高"（模式匹配级别）

Agent 负责（综合判断）：
  ✅ 基于 CogKOS 返回的结构化数据，用自己的 LLM 做综合推理
  ✅ 生成自然语言分析报告
  ✅ 做决策建议："建议不选这家供应商"
  ✅ 决策结果回传 CogKOS（经验回流）
```

**能力边界表**：

| 能力 | CogKOS | Agent |
|------|--------|-------|
| 存储和检索事实 | ✅ | — |
| 多来源聚合 + 置信度 | ✅ | — |
| 冲突标记 | ✅ | — |
| 间接关联发现（图扩散） | ✅ | — |
| 知识空洞检测 | ✅ | — |
| 轻量预测（模式匹配） | ✅ | — |
| **综合推理判断** | ❌ | ✅ |
| **生成自然语言分析** | ❌ | ✅ |
| **做决策和建议** | ❌ | ✅ |
| 积累过往判断 | ✅ 存储 | ✅ 产生并回传 |

**关键理解**：CogKOS 提供结构化原材料（事实+聚合+冲突+关联+空洞+预测），Agent 用自己的 LLM 将原材料综合为面向用户的最终回答。

**知识积累闭环**：如果 Agent 做过一次分析（"XX 公司供应链评分 B+"），这个判断作为 Assertion 回传 CogKOS 存储。下次查询时，CogKOS 直接返回该判断作为已有知识——系统因此越用越聪明，而不是每次从零分析。

---

## 第一部分：科学基础

### S1. 记忆的本质是预测，不是存档

**来源**：预测处理理论（Clark & Friston）

**工程推论**：MCP 返回结果应包含预测和置信度，而非只返回原始知识。

### S2. 快捕获/慢整合的互补学习

**来源**：海马-新皮层互补学习系统

**工程推论**：写入管道只做快轨道入库，Sleep-time 异步做聚合整合。

### S3. 读取即写入

**来源**：记忆再巩固理论（Nader）+ agent-episodic-memory skill

每次知识被检索，必须原子更新元数据：`activation_weight += Δ, access_count++, last_accessed = now`。

**工程推论**：MCP 查询不是纯读操作——每次查询都会修改知识的活跃度。高活跃度知识在衰减中存活更久，在聚合中优先级更高。

### S4. 知识有保质期

`effective_confidence(t) = confidence × e^(-λt)`，但被 `activation_weight` 调制——频繁使用的知识衰减更慢。

### S5. 进化的三要素：变异、选择、遗传

| 要素 | 实现 | 不是类比 |
|------|------|---------|
| **变异** | 冲突/新数据涌入 | ConflictRecord = 知识突变 |
| **选择** | 预测验证 + 衰减 | prediction_error 回写 |
| **遗传** | 高置信度知识派生 | derived_from + 贝叶斯聚合 |

### S6. 双路径认知

**来源**：Kahneman System 1/System 2 + skill 工程实现

| | System 1（Fast Path） | System 2（Slow Path） |
|---|---|---|
| 前端使用 | Agent 本地缓存：相同 context hash → 直接出结果 | Agent 调 MCP 做完整推理 |
| 后端使用 | MCP 查询缓存：高频查询直接返回 | 完整向量检索+图扩散+LLM预测 |

**工程推论**：MCP Server 维护查询结果缓存，缓存条目带置信度和命中/成功统计。Agent 反馈决策错误时，缓存置信度下降。

---

## 第二部分：工程原则

### E1. 所有外部输入都是断言（Assertion），不是事实

每条进入系统的知识携带 Claimant。系统通过聚合转化为信念。

### E2. 冲突是信号，不是错误

ConflictRecord 记录冲突。冲突积累触发 LLM 生成更高维度解释。

### E3. 权限内建于知识，不是外挂的

AccessEnvelope + 多租户数据库级隔离。

### E4. 删除有五种语义

GDPR擦除 / 合同终止 / 知识过期 / 去重 / 用户纠错。

### E5. 来源可追溯

ProvenanceRecord + 审计哈希。

### E6. 统一数据结构

EpistemicClaim + ConflictRecord。通过 `consolidation_stage` 区分阶段。

### E7. MCP 是第一接口

查询返回结构化的决策包：信念 + 冲突 + 预测 + 知识空洞 + 新鲜度。

### E8. 前后端分离

CogKOS 不运行 Agent，不做即时决策。Agent 拥有自己的操作记忆、决策引擎和结果缓存。MCP 是唯一边界。

### E10. Authority Tiers — Knowledge is Not Equal

Not all knowledge carries equal weight. Authority is derived from source, type, and epistemic status — not assigned manually. Five tiers (Canonical → Ephemeral) modulate query ranking, decay rates, and conflict resolution priority. Canonical knowledge (business policy, admin-maintained) never decays; ephemeral knowledge (working memory, RSS feeds) decays at 2x the base rate.

### E11. Layered Memory — Atkinson-Shiffrin for Agents

Three memory layers with distinct decay profiles: Working (session context, λ=0.5), Episodic (event memories, λ=0.05), Semantic (long-term knowledge, λ=0.01). Each layer has hard TTL boundaries. Working/episodic memories require explicit session scoping to access. Semantic is the default and the only layer that feeds into consolidation and evolution.

### E9. 计算预算分配

Sleep-time 任务有不同优先级和预算上限：

| 任务类型 | CPU 预算 | 频率 |
|---------|---------|------|
| 冲突检测 | < 5% | 每次写入 |
| 贝叶斯聚合 | < 10% | 周期批处理 |
| 衰减计算 | < 5% | 日级 |
| 冲突→Insight 提炼 | ≤ 30% | 低频（冲突密度超阈值时） |
| 范式刷新（C6） | ≤ 50% | 极低频（反常信号累积时） |

---

## 第三部分：进化目标

### G1. 知识进化（变异 + 选择 + 遗传）

### G2. 预测-反馈闭环

Agent → 查询预测 → 执行决策 → 结果回流 → 预测验证/证伪 → 置信度修正。

### G3. 联邦化群体智慧

多实例匿名化 Insight 共享 + 四条件量化健康检查（策略熵/基尼系数/级联检测/聚合质量）。

### G4. 双模式进化引擎

替代之前模糊的"C6实现"：
- **渐进模式（99%）**：贝叶斯聚合、衰减、小幅置信度调整
- **范式转换模式（1%）**：当反常信号累积超阈值 → LLM 沙箱生成新框架 → A/B 测试 → >10%提升才切换

反常信号 = 预测误差持续超阈值 + 冲突密度异常升高 + 查询缓存命中率持续下降。

### G5. 事务性记忆

CogKOS 维护元知识目录：不存储全部知识的全部细节，而是索引"哪个实例/哪个知识域最擅长什么"。跨实例查询时先路由到最合适的知识源。

---

## 设计灵感来源（诚实标注）

| 来源 | 用了什么 | 诚实程度 |
|------|---------|---------|
| 预测处理理论 | 记忆→预测, MCP返回预测 | ✅ 直接对应 |
| 互补学习系统 | 快/慢分离 | ✅ 直接对应 |
| 记忆再巩固理论 | 读即写, activation_weight | ✅ 直接工程化（CAS更新）|
| Kahneman Sys1/2 | 查询缓存 + 完整推理双路径 | ✅ 直接工程化 |
| 贝叶斯理论 | 置信度聚合 | ✅ 数学直接使用 |
| 达尔文进化论 | 变异+选择+遗传 | ✅ 直接对应 |
| DIKW | 三层升维+计算预算 | ✅ 直接工程化 |
| Surowiecki 群体智慧 | 四条件+量化检测 | ✅ 直接工程化 |
| 事务性记忆 | 元知识目录 | ✅ 直接工程化 |
| 佛教般若 | 框架可替换性 | 🟡 远期灵感 |
