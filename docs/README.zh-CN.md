# CogKOS — AI 智能体的长期记忆

[![CI](https://github.com/Kingxiao/cogkos/actions/workflows/ci.yml/badge.svg)](https://github.com/Kingxiao/cogkos/actions)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](../LICENSE-APACHE)
[![Rust](https://img.shields.io/badge/rust-1.94+-orange.svg)](https://www.rust-lang.org)

**[English](../README.md) | 中文**

让你的 AI Agent 拥有跨会话、跨 Agent 的持久记忆。本地部署，开源，通过 [MCP 协议](https://modelcontextprotocol.io/) 接入。

你的 Agent 每次新会话都从零开始。CogKOS 解决这个问题——它记住 Agent 学到的东西，下次自动找到相关知识，过时的知识自动淡出。上传你的公司资料，所有 Agent 都能用，不用你反复说。

> 别再对你的 Agent 重复同样的话了。

## 为什么需要 CogKOS

| 痛点 | CogKOS 怎么解决 |
|------|----------------|
| **Agent 换个会话就失忆** | 持久记忆——重启、新会话都不丢 |
| **Agent 之间互不知道对方学了什么** | 一个 Agent 的发现，其他 Agent 自动可用 |
| **上下文窗口被"提醒"塞满** | 语义搜索只召回相关知识，不浪费 token |
| **公司文档和 Agent 猜测同等对待** | 五级知识权威性——公司政策永远排在 Agent 猜测前面 |
| **笔记越积越多没人维护** | 置信度自动衰减，矛盾自动标记 |
| **不想把知识存到云端** | 本地部署 BGE-M3 embedding，零 API 费用 |

## 快速开始

### 环境要求

- **Rust**: 1.94+ (edition 2024)
- **Docker & Docker Compose**
- **NVIDIA GPU**（可选，加速 embedding）

### 1. 一键启动

```bash
git clone https://github.com/Kingxiao/cogkos.git && cd cogkos
docker-compose up -d                              # PostgreSQL + FalkorDB
docker compose -f docker-compose.bge-m3.yml up -d # 本地 BGE-M3 embedding（GPU）
cp .env.example .env
cargo build --release
./target/release/cogkos &
```

> 没有 GPU？用 `docker compose -f docker-compose.bge-m3.yml --profile cpu up -d`。
> 在 `.env` 中设置 `DEFAULT_MCP_API_KEY=any-string` 可跳过 API Key 创建。

### 2. 接入你的 Agent

以 Claude Code 为例，编辑 `~/.claude/mcp_servers.json`：

```json
{
  "cogkos": {
    "type": "streamable-http",
    "url": "http://localhost:3000/mcp",
    "headers": { "X-API-Key": "your-key" }
  }
}
```

### 3. 开始使用

```python
from cogkos import CogKOS

brain = CogKOS("http://localhost:3000/mcp", api_key="your-key", tenant_id="my-project")

brain.learn("我们的 API 使用 bcrypt 做密码哈希，禁止 md5", confidence=0.95)
result = brain.recall("应该用什么哈希算法？")
brain.feedback(result.query_hash, success=True)
```

Agent 接入后自动获得以下 MCP 工具：

| 工具 | 做什么 |
|------|-------|
| `query_knowledge` | 语义搜索 + 图遍历——找到相关知识 |
| `submit_experience` | 存储学习成果、决策或观察 |
| `submit_feedback` | 告诉 CogKOS 回答是否有用（调整置信度） |
| `upload_document` | 上传文档（PDF、Word、Excel、CSV、图片等） |
| `report_gap` | 标记知识空白，引导定向采集 |
| `get_meta_directory` | 浏览知识领域和专业度评分 |

## 核心能力

### 知识权威性分级

不是所有知识都一样重要。CogKOS 自动分类和排序：

| 等级 | 是什么 | 衰减速度 | 查询优先级 |
|------|--------|---------|-----------|
| **T1 教规级** | 公司政策、管理员上传的文档 | 永不衰减 | 最高 |
| **T2 策展级** | 上传的参考文档 | 极慢 | 高 |
| **T3 验证级** | 经过反馈确认的 Agent 知识 | 慢 | 中 |
| **T4 观察级** | Agent 的发现（默认） | 正常 | 标准 |
| **T5 临时级** | 工作记忆、RSS 新闻 | 快 | 最低 |

公司政策和 Agent 猜测矛盾时，系统自动建议以政策为准。

### 文档摄入

上传文档，CogKOS 自动提取结构化知识：

| 格式 | 支持 |
|------|------|
| PDF、Word (.docx)、PowerPoint (.pptx) | 完整解析 + 语义分块 |
| Excel (.xlsx)、CSV、TSV | 按行分块，保留列头上下文 |
| Markdown、HTML、JSON、XML、YAML | 原生文本解析 |
| 图片 (PNG、JPG) | 基于 LLM 视觉的文字提取 |

文档按语义边界（段落、章节）分块，不是固定字符窗口。配置了 LLM 后，自动提取关键事实、决策和预测。

### 三层记忆

| 层 | 范围 | 生命周期 | 共享？ |
|---|------|---------|--------|
| **语义层** | 租户范围 | 数月 | 是——所有 Agent |
| **情节层** | 单个 Agent | 数天 | 否——仅该 Agent |
| **工作层** | 单次会话 | 数小时 | 否——仅该会话 |

默认查询只搜语义层。Agent 的草稿笔记不会泄漏到其他 Agent 的结果中。

## 工作原理

```
L7  摄入管道    — PDF/Word/Excel/CSV/JSON/XML/HTML/图片 + LLM 提取
L6  MCP 服务器  — 认证、缓存、语义搜索、图扩散
L5  知识图谱    — 知识节点、关系、冲突记录 (FalkorDB)
L4  进化引擎    — 权威性感知衰减、贝叶斯聚合、冲突解决
L3  后台处理    — 14 个调度任务，带熔断器
L2  外部知识    — RSS/Webhook/API 轮询
L1  持久化      — PostgreSQL + pgvector / FalkorDB / S3
```

## Embedding 模型

CogKOS 支持任何 OpenAI 兼容的 embedding API。默认使用本地 BGE-M3，无需 API Key。

| 模型 | 维度 | 费用 | 启动方式 |
|------|------|------|---------|
| **BGE-M3（本地 GPU）** | 1024 | 免费 | `docker compose -f docker-compose.bge-m3.yml up -d` |
| BGE-M3（本地 CPU） | 1024 | 免费 | `docker compose -f docker-compose.bge-m3.yml --profile cpu up -d` |
| BGE-M3（DeepInfra） | 1024 | ~$0.01/1M tokens | 在 `.env` 设置 `EMBEDDING_API_KEY` |
| text-embedding-3-large | 3072 | ~$0.13/1M tokens | 在 `.env` 设置 `OPENAI_API_KEY` |

## Python SDK

```bash
pip install cogkos  # 或: cd sdk/python && pip install -e .
```

```python
from cogkos import CogKOS

brain = CogKOS("http://localhost:3000/mcp", api_key="key", tenant_id="my-project")
brain.learn("Rust 通过 borrow checker 实现内存安全", confidence=0.9)
result = brain.recall("Rust 如何处理内存？")
print(result.best_belief)
```

## 项目结构

```
crates/
├── cogkos-core/       数据模型、权威性分级、RBAC、进化引擎
├── cogkos-store/      PostgreSQL + pgvector + FalkorDB + S3 存储
├── cogkos-mcp/        MCP 服务器、查询/摄入/反馈处理器
├── cogkos-ingest/     文档解析、语义分块、LLM 知识提取
├── cogkos-sleep/      14 任务后台调度器（带熔断器）
├── cogkos-llm/        多供应商 LLM 客户端
├── cogkos-external/   RSS/Webhook/API 轮询
└── cogkos-federation/ 群体智慧健康检查（部分）
sdk/python/            Python SDK
```

## 开发

```bash
cargo test          # 600+ 个测试
cargo fmt           # 格式化
cargo clippy        # 静态检查
```

## 部署

```bash
# 一键启动（开发模式）
docker compose -f docker-compose.quickstart.yml up -d

# 生产部署
docker build -t cogkos:latest .
docker run -d -p 3000:3000 -p 8081:8081 --env-file .env cogkos:latest

# 健康检查
curl http://localhost:8081/healthz   # → "ok"
curl http://localhost:8081/readyz    # → "ready"
```

## 许可证

基于 [Apache-2.0](../LICENSE-APACHE) 协议开源。

## 相关链接

- [架构设计](ARCHITECTURE.md)
- [API 规范](API_SPEC.md)
- [设计原则](PRINCIPLES.md)
- [路线图](ROADMAP.md)
