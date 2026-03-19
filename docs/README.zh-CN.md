# CogKOS — 认知知识操作系统

[![CI](https://github.com/Kingxiao/cogkos/actions/workflows/ci.yml/badge.svg)](https://github.com/Kingxiao/cogkos/actions)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](../LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.94+-orange.svg)](https://www.rust-lang.org)

**[English](../README.md) | 中文**

CogKOS (Cognitive Knowledge Operating System) 是一个专为 AI Agent 设计的**认知知识后端**——通过 [MCP 协议](https://modelcontextprotocol.io/) 为 Agent 提供长期记忆、知识进化和预测能力。

> Agent 负责决策，CogKOS 负责记住*为什么*。

## 核心特性

| 特性 | 描述 |
|------|------|
| **七层架构** | 从持久化到摄入管道的完整分层，每层职责清晰 |
| **MCP 协议** | 基于 rmcp SDK，支持 stdio 和 Streamable HTTP 双传输 |
| **双模式进化** | 渐进模式 (99%) 持续优化 + 范式转换模式 (1%) 突破创新 |
| **多租户隔离** | 数据库级别 RLS，租户数据完全隔离 |
| **Sleep-time 计算** | 异步知识整合、置信度衰减、冲突检测与解决 |
| **知识图谱** | FalkorDB 图数据库 + 激活扩散机制 |
| **语义检索** | PostgreSQL pgvector，运行时动态检测向量维度 |
| **异步写入** | PG 快写 (~1ms 返回)，embedding + 索引后台异步完成（S2 原则） |

## 快速开始

### 环境要求

- **Rust**: 1.94+ (edition 2024)
- **PostgreSQL**: 17+ (需要 pgvector 扩展)
- **FalkorDB**: Redis 协议兼容
- **Docker & Docker Compose**: 推荐

### 1. 克隆并启动基础设施

```bash
git clone https://github.com/Kingxiao/cogkos.git
cd cogkos

# 启动 PostgreSQL (pgvector) + FalkorDB
docker-compose up -d
```

### 2. 配置并构建

```bash
cp .env.example .env
# 编辑 .env — 参考注释了解必填/选填项

cargo build --release
```

### 3. 首次启动 — 初始化并创建 API Key

```bash
# 启动服务（自动执行数据库迁移）
./target/release/cogkos &

# 创建首个 API Key（tenant = 你的组织/项目名）
./target/release/cogkos-admin create-key my-org read,write
# 输出: API Key: ck_xxxxxxxxxxxx（仅显示一次，请保存）

# 验证
curl http://localhost:8081/healthz   # → "ok"
curl http://localhost:8081/readyz    # → "ready"（检查 PG + FalkorDB）
```

> **快速开发模式**: 在 `.env` 中设置 `DEFAULT_MCP_API_KEY=any-string` 可跳过 admin CLI 创建密钥。

### 4. 连接你的 Agent

CogKOS 采用 **租户/Agent** 模型：
- **租户 (Tenant)** = 你的组织（数据隔离边界）
- **Agent** = 同一租户内共享知识池的多个 AI Agent

```json
// Agent A — Claude Code (~/.claude/mcp_servers.json)
{
  "cogkos": {
    "type": "streamable-http",
    "url": "http://localhost:3000/mcp",
    "headers": {
      "X-API-Key": "ck_xxxxxxxxxxxx"
    }
  }
}

// Agent B — 同一租户，不同 Key（或相同 Key）
// 租户绑定在 API Key 上，无需额外 header
```

同一租户内的所有 Agent：
- **共享**同一个知识图谱和语义搜索索引
- **跨 Agent 冲突检测**（Agent A 说 X，Agent B 说非 X → 自动检测）
- 通过请求中的 `source.agent_id` 标识身份（用于溯源追踪）

## 架构

```
L7  摄入管道 — PDF/Word/Markdown 解析 + LLM 分类
L6  MCP 服务器 (rmcp SDK) — 认证、缓存、激活扩散
L5  知识图谱 (FalkorDB) — EpistemicClaim 节点、关系、冲突记录
L4  进化引擎 — 渐进进化 (99%) + 范式转换 (1%)
L3  异步整合 (Sleep-time) — 冲突检测、贝叶斯聚合、衰减
L2  外部知识 — RSS/Webhook/API 轮询
L1  持久化 — PostgreSQL + pgvector / FalkorDB / S3
```

## MCP 工具

| 工具 | 描述 |
|------|------|
| `query_knowledge` | 语义搜索 + 图扩散，返回结构化决策包 |
| `submit_experience` | 提交知识，异步 embedding + 冲突检测 |
| `submit_feedback` | 反馈回路——70/30 混合比例调整置信度 |
| `report_gap` | 报告知识空洞，引导定向采集 |
| `upload_document` | 上传文档触发摄入管道 |
| `get_meta_directory` | 浏览知识领域和专业度评分 |
| `subscribe_rss` / `subscribe_webhook` / `subscribe_api` | 外部知识源订阅 |

## Crate 结构

```
crates/
├── cogkos-core/       核心模型、RBAC、进化引擎、健康监控
├── cogkos-store/      PostgreSQL + pgvector + FalkorDB + S3 存储抽象
├── cogkos-mcp/        MCP 服务器、查询/摄入/反馈处理器
├── cogkos-ingest/     文档解析 + 向量化管道
├── cogkos-sleep/      异步任务调度（冲突/衰减/聚合）
├── cogkos-llm/        多供应商 LLM 客户端
├── cogkos-external/   RSS/Webhook/API 轮询
├── cogkos-federation/ 跨实例路由（实验性）
└── cogkos-workflow/   工作流引擎（占位）
```

## 设计原则

| # | 原则 | 含义 |
|---|------|------|
| S1 | 记忆本质是预测 | 查询返回包含预测和置信度的决策包 |
| S2 | 快捕获/慢整合 | 写入：同步 PG 插入，异步 embedding + 索引 |
| S3 | 读即写 | 查询命中时原子更新激活权重 |
| S4 | 知识有保质期 | `confidence × e^(-λt)`，被激活权重调制 |
| S5 | 进化三要素 | 变异 (冲突) + 选择 (衰减) + 遗传 (贝叶斯聚合) |
| S6 | 双路径认知 | System 1 (缓存) + System 2 (完整推理) |

## 技术栈

| 组件 | 技术 |
|------|------|
| 语言 | Rust 1.94+ (edition 2024) |
| 关系库 | PostgreSQL 17 + pgvector (HNSW 索引) |
| 图数据库 | FalkorDB (Redis 协议) |
| 对象存储 | S3 / SeaweedFS / 本地文件系统降级 |
| MCP | rmcp SDK 1.2+ (stdio + Streamable HTTP) |
| 监控 | Prometheus + OpenTelemetry + JSON 日志 |

## 开发

```bash
cargo test          # 单元测试 (69 tests)
cargo fmt           # 格式化
cargo clippy        # 静态检查
cargo audit         # 安全审计
```

## 部署

```bash
# Docker
docker build -t cogkos:latest .
docker run -d -p 3000:3000 -p 8081:8081 --env-file .env cogkos:latest

# Kubernetes
kubectl apply -k k8s/overlays/dev
```

## 许可证

双许可：[MIT](../LICENSE-MIT) 或 [Apache-2.0](../LICENSE-APACHE)。

## 相关链接

- [架构设计](ARCHITECTURE.md)
- [API 规范](API_SPEC.md)
- [设计原则](PRINCIPLES.md)
- [路线图](ROADMAP.md)
