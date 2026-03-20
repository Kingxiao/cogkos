# CogKOS — AI 智能体的长期记忆

[![CI](https://github.com/Kingxiao/cogkos/actions/workflows/ci.yml/badge.svg)](https://github.com/Kingxiao/cogkos/actions)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](../LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.94+-orange.svg)](https://www.rust-lang.org)

**[English](../README.md) | 中文**

让你的 AI Agent 拥有跨会话、跨 Agent 的持久记忆。本地部署，开源，通过 [MCP 协议](https://modelcontextprotocol.io/) 接入。

你的 Agent 每次新会话都从零开始。CogKOS 解决这个问题——它记住 Agent 学到的东西，下次自动找到相关知识，过时的知识自动淡出，不用你手动维护。

> 别再对你的 Agent 重复同样的话了。

## 为什么需要 CogKOS

| 痛点 | CogKOS 怎么解决 |
|------|----------------|
| **Agent 换个会话就失忆** | 持久记忆——重启、新会话都不丢 |
| **Agent 之间互不知道对方学了什么** | 一个 Agent 的发现，其他 Agent 自动可用 |
| **上下文窗口被"提醒"塞满** | 语义搜索只召回相关知识，不浪费 token |
| **笔记越积越多没人维护** | 置信度自动衰减，矛盾自动标记 |
| **不想把知识存到云端** | 跑在你自己的机器上，数据不出本地 |
| **临时笔记污染长期知识** | 三层隔离——会话草稿、Agent 经历、共享长期记忆 |

## 快速开始

### 环境要求

- **Rust**: 1.94+ (edition 2024)
- **Docker & Docker Compose**

### 1. 启动基础设施并构建

```bash
git clone https://github.com/Kingxiao/cogkos.git && cd cogkos
docker-compose up -d        # PostgreSQL (pgvector) + FalkorDB
cp .env.example .env        # 按需编辑
cargo build --release
./target/release/cogkos &   # 自动执行数据库迁移
```

> 在 `.env` 中设置 `DEFAULT_MCP_API_KEY=any-string` 可跳过 API Key 创建，快速开始开发。

### 2. 接入你的 Agent

以 Claude Code 为例，编辑 `~/.claude/mcp_servers.json`：

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

### 3. 开始使用

Agent 接入后自动获得以下 MCP 工具：

| 工具 | 做什么 |
|------|-------|
| `query_knowledge` | 检索相关知识——语义搜索 + 图遍历 |
| `submit_experience` | 存储学习成果、决策或观察 |
| `submit_feedback` | 告诉 CogKOS 回答是否有用（调整置信度） |
| `report_gap` | 标记知识空白，引导定向采集 |
| `upload_document` | 上传文档进入摄入管道 |
| `get_meta_directory` | 浏览知识领域和专业度评分 |

这次会话学到的东西，下次会话还在。

## 工作原理

```
L7  摄入管道    — PDF/Word/Markdown 解析 + LLM 分类
L6  MCP 服务器  — 认证、缓存、语义搜索、图扩散
L5  知识图谱    — 知识节点、关系、冲突记录 (FalkorDB)
L4  进化引擎    — 置信度衰减、贝叶斯聚合、冲突解决
L3  后台处理    — 异步 embedding、整合、垃圾回收
L2  外部知识    — RSS/Webhook/API 轮询
L1  持久化      — PostgreSQL + pgvector / FalkorDB / S3
```

**核心概念：**

- **EpistemicClaim** — 知识的原子单元。包含内容、置信度、来源和激活权重。
- **三层记忆** — Working（会话草稿，自动过期）、Episodic（Agent 经历）、Semantic（共享长期知识）。查询默认只搜语义层，Agent 的工作记忆不会泄漏到其他 Agent 的结果中。
- **置信度衰减** — `confidence × e^(-λt)`，被使用频率调制。过时知识自动淡出，常用知识保持活跃。
- **冲突检测** — 两条知识矛盾时，CogKOS 标记冲突等待解决，而不是静默丢弃一方。

## 项目结构

```
crates/
├── cogkos-core/       数据模型、RBAC、健康监控
├── cogkos-store/      PostgreSQL + pgvector + FalkorDB + S3 存储抽象
├── cogkos-mcp/        MCP 服务器、查询/摄入/反馈处理器
├── cogkos-ingest/     文档解析 + 向量化管道
├── cogkos-sleep/      后台任务调度（衰减、聚合）
├── cogkos-llm/        多供应商 LLM 客户端
├── cogkos-external/   RSS/Webhook/API 轮询
└── cogkos-federation/ 跨实例路由（实验性）
```

## 配置

关键环境变量（完整列表见 `.env.example`）：

| 变量 | 用途 |
|------|------|
| `DATABASE_URL` | PostgreSQL 连接字符串 |
| `FALKORDB_URL` | FalkorDB（Redis 协议）连接 |
| `API_302_KEY` 或 `OPENAI_API_KEY` | 语义搜索的 Embedding 提供商 |
| `DEFAULT_MCP_API_KEY` | 本地开发跳过 API Key 创建 |
| `MCP_TRANSPORT` | `http` 启用 Streamable HTTP（默认 stdio） |

## 开发

```bash
cargo test          # 69 个测试
cargo fmt           # 格式化
cargo clippy        # 静态检查
```

## 部署

```bash
# Docker
docker build -t cogkos:latest .
docker run -d -p 3000:3000 -p 8081:8081 --env-file .env cogkos:latest

# Kubernetes
kubectl apply -k k8s/overlays/dev

# 健康检查
curl http://localhost:8081/healthz   # → "ok"
curl http://localhost:8081/readyz    # → "ready"
```

## 许可证

双许可：[MIT](../LICENSE-MIT) 或 [Apache-2.0](../LICENSE-APACHE)。

## 相关链接

- [架构设计](ARCHITECTURE.md)
- [API 规范](API_SPEC.md)
- [设计原则](PRINCIPLES.md)
- [路线图](ROADMAP.md)
