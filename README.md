# CogKOS - Cognitive Knowledge Operating System

[![CI](https://github.com/Kingxiao/cogkos/actions/workflows/ci.yml/badge.svg)](https://github.com/Kingxiao/cogkos/actions)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org)

CogKOS (Cognitive Knowledge Operating System) 是一个专为 AI Agent 设计的认知知识操作系统，提供长期记忆、知识进化和预测能力。

> 🧠 **核心设计理念**: 让 AI Agent 拥有真正的长期记忆和知识进化能力，通过 Sleep-time 计算实现知识的自主整合与优化。

## 🌟 核心特性

| 特性 | 描述 |
|------|------|
| **七层架构设计** | 从持久化到摄入管道的完整分层架构，每层职责清晰 |
| **MCP 协议支持** | 基于 rmcp SDK 的 MCP 标准协议，支持 stdio 和 Streamable HTTP |
| **双模式进化** | 渐进模式 (99%) 持续优化 + 范式转换模式 (1%) 突破创新 |
| **多租户隔离** | 数据库级别的租户隔离，确保数据安全 |
| **Sleep-time 计算** | 异步知识整合、衰减和冲突解决 |
| **图神经网络** | 基于 FalkorDB 的知识图谱和激活扩散 |
| **向量检索** | PostgreSQL pgvector 语义向量检索 |
| **云原生设计** | Kubernetes 原生部署，支持自动扩缩容 |

## 🚀 快速开始

### 环境要求

- **Rust**: 1.94+ (项目使用 edition 2024)
- **PostgreSQL**: 16+ (with pgvector extension)
- **FalkorDB**: (Redis 协议兼容)
- **Docker & Docker Compose**: (推荐用于本地开发)

### 1. 克隆仓库

```bash
git clone https://github.com/Kingxiao/cogkos.git
cd cogkos
```

### 2. 启动依赖服务

```bash
# 使用 Docker Compose 一键启动所有依赖
docker-compose up -d

# 检查服务状态
docker-compose ps
```

这将启动：
- PostgreSQL with pgvector (端口 5432) - 主数据库 + 向量检索
- FalkorDB (端口 6379) - 图数据库
- SeaweedFS (端口 9333 Master / 8080 Volume / 8333 S3 / 8888 Filer) - 分布式对象存储

### 3. 数据库迁移

```bash
# 安装 sqlx-cli
cargo install sqlx-cli --no-default-features --features postgres

# 运行迁移
sqlx migrate run
```

### 4. 配置环境变量

```bash
# 复制环境变量模板
cp .env.example .env

# 编辑 .env 文件，根据需要修改配置
vim .env
```

### 5. 构建并运行

```bash
# 开发模式构建
cargo build

# 运行服务
cargo run

# 或使用 release 模式
 cargo build --release
./target/release/cogkos
```

服务将在 `http://localhost:3000` 启动。

### 6. 验证安装

```bash
# 测试健康检查
curl http://localhost:3000/health

# 测试 MCP 查询 (需要 API Key)
curl -X POST http://localhost:3000/mcp/query \
  -H "Content-Type: application/json" \
  -H "X-API-Key: your-api-key" \
  -H "X-Tenant-ID: default" \
  -d '{
    "query": "测试查询",
    "context": {"max_results": 5}
  }'
```

## 🏗️ 架构设计

### 七层架构

```
┌─────────────────────────────────────────────────────────────────┐
│  L7: 摄入管道层 (Ingestion Pipeline)                              │
│  文档解析 → 粗分类 → 知识提取 → 向量化                             │
├─────────────────────────────────────────────────────────────────┤
│  L6: 查询层 (MCP Server via rmcp SDK)                            │
│  MCP 标准协议、认证、缓存、激活扩散                                │
├─────────────────────────────────────────────────────────────────┤
│  L5: 知识图谱层 (Graph Layer)                                     │
│  FalkorDB 存储、关系推理、激活扩散                                  │
├─────────────────────────────────────────────────────────────────┤
│  L4: 进化引擎层 (Evolution Engine)                                │
│  渐进进化、范式转换、异常信号检测                                   │
├─────────────────────────────────────────────────────────────────┤
│  L3: 异步整合层 (Sleep-time Compute)                              │
│  知识聚合、置信度衰减、冲突检测                                     │
├─────────────────────────────────────────────────────────────────┤
│  L2: 外部知识层 (External Knowledge)                              │
│  RSS 订阅、Webhook 接收                                          │
├─────────────────────────────────────────────────────────────────┤
│  L1: 持久化层 (Persistence)                                       │
│  PostgreSQL + pgvector / FalkorDB / S3                           │
└─────────────────────────────────────────────────────────────────┘
```

Note: 联邦层 (Federation) V1 中已冻结，不在主架构图中显示。

### 系统架构图

```
┌─ Agent 前端层 ──────────────────────┐       ┌─ CogKOS 后端 ──────────────────────────┐
│                                     │       │                                        │
│  Agent A ─┐                         │       │   ┌─────────────────────────────────┐  │
│  Agent B ─┤── 决策引擎              │ MCP   │   │ 摄入管道 (L7)                    │  │
│  Agent C ─┘   操作记忆(本地)         │ ←───→ │   │ MCP 服务器 (L6) ←→ rmcp SDK    │  │
│               查询缓存(本地)         │       │   │ 知识图谱 (L5) ←→ FalkorDB       │  │
│                                     │       │   │ 进化引擎 (L4)                    │  │
│  Agent 负责：即时决策、操作记忆       │       │   │ 异步整合 (L3) ←→ NATS           │  │
│  Agent 不负责：长期记忆、知识进化     │       │   │ 外部知识 (L2)                    │  │
└─────────────────────────────────────┘       │   │ 持久化层 (L1) ←→ PG+pgvector/S3 │  │
                                              │   └─────────────────────────────────┘  │
                                              │                                        │
                                              └─────────────────────────────────────────┘
```

### 数据流

```
┌─────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│  Agent  │───→│  MCP Server │───→│  查询引擎    │───→│  向量检索    │
└─────────┘    └─────────────┘    └─────────────┘    └──────┬──────┘
     ↑                           ↓                         │
     │                    ┌─────────────┐                   │
     └────────────────────│  知识图谱    │←──────────────────┘
                          └─────────────┘
                                ↓
                          ┌─────────────┐
                          │ Sleep-time  │
                          │  整合/衰减   │
                          └─────────────┘
```

## 💻 开发指南

### 项目结构

```
cogkos/
├── crates/                    # Rust workspace crates
│   ├── cogkos-core/          # 核心模型和领域逻辑
│   ├── cogkos-store/         # 存储抽象层 (PostgreSQL, pgvector, FalkorDB, S3)
│   ├── cogkos-mcp/           # MCP 服务器实现
│   ├── cogkos-ingest/        # 摄入管道
│   ├── cogkos-sleep/         # 异步任务调度
│   ├── cogkos-llm/           # LLM 客户端
│   ├── cogkos-federation/    # 联邦学习
│   ├── cogkos-external/      # 外部知识源
│   └── cogkos-workflow/      # 工作流引擎
├── src/                      # 主程序入口
├── tests/                    # 集成测试
├── migrations/               # 数据库迁移
├── k8s/                      # Kubernetes 配置
│   ├── base/                 # 基础资源
│   └── overlays/             # 环境覆盖 (dev/staging/prod)
├── charts/                   # Helm Charts
└── docs/                     # 文档
    ├── ARCHITECTURE.md       # 详细架构设计
    ├── API_SPEC.md           # API 规范
    ├── DATA_MODELS.md        # 数据模型
    └── PROJECT_SETUP.md      # 项目设置
```

### 运行测试

```bash
# 单元测试
cargo test --lib

# 集成测试 (需要 Docker 环境)
docker-compose up -d
cargo test --test '*'

# 代码覆盖率
cargo tarpaulin --out Html
```

### 代码规范

```bash
# 格式化
cargo fmt

# 静态检查
cargo clippy --all-targets --all-features

# 安全检查
cargo audit

# 生成文档
cargo doc --open
```

## 📦 部署

### Docker 部署

```bash
# 构建镜像
docker build -t cogkos:latest .

# 运行容器
docker run -d \
  --name cogkos \
  -p 3000:3000 \
  --env-file .env \
  cogkos:latest
```

### Kubernetes 部署

```bash
# 使用 Kustomize
cd k8s/overlays/dev
kubectl apply -k .

# 或使用 Helm
helm install cogkos ./charts/cogkos \
  -n cogkos \
  --create-namespace \
  --set image.tag=latest
```

### 生产环境注意事项

1. **数据库**: 使用托管 PostgreSQL (如 AWS RDS, GCP Cloud SQL)
2. **对象存储**: 使用云厂商 S3 兼容服务
3. **监控**: 配置 Prometheus + Grafana
4. **日志**: 配置集中式日志收集 (如 ELK, Loki)
5. **TLS**: 使用 cert-manager 自动管理证书

## 📚 API 文档

### MCP 工具

CogKOS 提供以下 MCP 工具：

| 工具名 | 描述 |
|--------|------|
| `query_knowledge` | 查询知识库，返回结构化决策包 |
| `submit_experience` | Agent 推送经验/观察 |
| `submit_feedback` | 对查询结果的成功/失败反馈 |
| `report_gap` | 主动报告发现的知识空洞 |
| `get_meta_directory` | 查询元知识目录（联邦） |
| `upload_document` | 上传文档触发摄入管道 |

### 示例请求

```bash
# 查询知识
curl -X POST http://localhost:3000/mcp/query \
  -H "Content-Type: application/json" \
  -H "X-API-Key: your-api-key" \
  -H "X-Tenant-ID: your-tenant" \
  -d '{
    "query": "竞品X对中小企业的适用性",
    "context": {
      "domain": "竞品分析",
      "urgency": "normal",
      "max_results": 10
    },
    "include_predictions": true,
    "include_conflicts": true
  }'
```

更多 API 详情请参阅 [API 规范文档](docs/API_SPEC.md)。

## 🛠️ 技术栈

| 层级 | 组件 | 技术 |
|------|------|------|
| MCP Server | MCP 协议层 | Rust + rmcp SDK (stdio / Streamable HTTP) |
| 关系数据库 | 元数据、审计日志 | PostgreSQL 17 |
| 向量检索 | 语义检索 | PostgreSQL pgvector |
| 图数据库 | 知识图谱、关系存储 | FalkorDB |
| 对象存储 | 原始文档存储 | S3 / SeaweedFS |

## 🤝 贡献

欢迎提交 Issue 和 PR！请阅读 [贡献指南](CONTRIBUTING.md)。

### 开发流程

1. Fork 仓库
2. 创建功能分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m 'Add amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 创建 Pull Request

## 📄 许可证

本项目采用 MIT OR Apache-2.0 双许可证。

- [MIT 许可证](LICENSE-MIT)
- [Apache 2.0 许可证](LICENSE-APACHE)

## 🔗 相关链接

- [架构设计](docs/ARCHITECTURE.md)
- [API 规范](docs/API_SPEC.md)
- [运维手册](docs/OPS_GUIDE.md)

---

<p align="center">
  Built with Rust
</p>
