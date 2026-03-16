# CogKOS 模块开发索引
生成时间：2026-03-10 14:56

## 代码结构

```
cogkos/
├── crates/
│   ├── cogkos-core/       # 核心数据模型（EpistemicClaim, EvolutionEngine）
│   ├── cogkos-store/      # 存储层（PostgreSQL/pgvector, FalkorDB, S3）
│   ├── cogkos-mcp/        # MCP Server（L6 查询/接口层）
│   │   └── src/tools/    # MCP 工具实现
│   │       ├── query.rs   # query_knowledge
│   │       ├── submit.rs  # submit_experience
│   │       ├── feedback.rs # submit_feedback
│   │       ├── upload.rs  # upload_document
│   │       ├── gap.rs     # report_gap
│   │       └── meta.rs    # get_meta_directory
│   ├── cogkos-ingest/     # 摄入管道（L7）
│   │   └── src/
│   │       ├── parser/    # PDF/Word/Markdown 解析器
│   │       ├── pipeline.rs
│   │       └── classifier.rs
│   └── cogkos-sleep/      # Sleep-time 调度（L3/L2）
│       └── src/scheduler.rs
├── docs/                   # 架构文档
├── docker-compose.yml       # 开发环境
└── .github/workflows/      # CI/CD
```

## Issue 与代码位置映射

| Issue | 标题 | 主要代码文件 | 依赖 |
|-------|------|-------------|------|
| #91 | 图上激活扩散核心 | `crates/cogkos-mcp/src/tools/query.rs` | 无 |
| #92 | 激活扩散性能优化 | `crates/cogkos-mcp/src/tools/query.rs` | #91 |
| #93 | 激活扩散与向量检索合并 | `crates/cogkos-mcp/src/tools/query.rs` | #91 |
| #94 | 预测结果生成 | `crates/cogkos-mcp/src/tools/query.rs` | #93 |
| #95 | 测试编译错误修复 | `tests/` | 无 |
| #96 | submit_feedback 缓存更新 | `crates/cogkos-mcp/src/tools/feedback.rs` | 无 |
| #97 | upload_document 管道编排 | `crates/cogkos-mcp/src/tools/upload.rs` | 无 |
| #98 | PDF 解析集成 | `crates/cogkos-ingest/src/parser/pdf.rs` | #97 |
| #99 | Word 解析集成 | `crates/cogkos-ingest/src/parser/docx.rs` | #97 |
| #100 | S3 存储集成 | `crates/cogkos-store/src/s3.rs` | #97 |
| #101 | report_gap 知识空洞 | `crates/cogkos-mcp/src/tools/gap.rs` | 无 |
| #102 | get_meta_directory | `crates/cogkos-mcp/src/tools/meta.rs` | 无 |
| #103 | Webhook 订阅 | `crates/cogkos-sleep/src/webhook.rs` | 无 |
| #104 | RSS 订阅 | `crates/cogkos-sleep/src/rss.rs` | 无 |
| #105 | API 轮询 | `crates/cogkos-sleep/src/api_poll.rs` | 无 |
| #106-#108 | 集成测试 | `tests/integration/` | 对应功能完成后 |
| #109 | 联邦路由 | `crates/cogkos-federation/` | #105 |
| #110 | 群体智慧量化 | `crates/cogkos-federation/` | 无 |
| #111 | V2 分布式缓存 | 架构设计 | 无 |
| #112 | V2 写入异步化 | 架构设计 | 无 |

## 开发流程

### 本地开发

```bash
# 1. 代码检查（本地只能做这个）
cd cogkos
cargo check

# 2. 推送触发 CI 编译
git add .
git commit -m "feat: #91 实现图上激活扩散"
git push origin main
```

### CI 编译

- **触发方式**: Push 到 main 分支或 PR
- **编译命令**: `.github/workflows/ci.yml` 中的 `cargo build --workspace`
- **查看结果**: https://github.com/Kingxiao/cogkos/actions

### 验收标准

每个 Issue 的验收标准见 `test-plan.md` 中对应的测试用例：
- 测试用例通过 = Issue 验收完成
- 测试用例格式: `TC-L6-01` = L6 模块第 1 个用例

## 环境依赖

| 服务 | 端口 | 用途 |
|------|------|------|
| PostgreSQL (含 pgvector) | 5432 | 元数据存储 + 向量检索 |
| FalkorDB | 6379 | 图数据库 |
| MinIO | 9000/9001 | S3 兼容存储 |
| NATS | 4222 | 消息队列（V2） |

启动开发环境:
```bash
cd cogkos
docker compose up -d
```
