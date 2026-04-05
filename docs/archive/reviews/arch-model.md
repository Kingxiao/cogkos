# CogKOS 架构模型
生成时间：2026-03-10 14:30

## 核心模块

| 模块名 | 职责描述 | 技术栈 |
|--------|---------|--------|
| **L1 持久化层** | 多存储后端：PostgreSQL(元数据)、FalkorDB(图谱)、Qdrant(向量)、TimescaleDB(时序)、S3(对象) | Rust + 各客户端库 |
| **L2 进化引擎层** | 双模式进化引擎：渐进模式(贝叶斯聚合/衰减/验证) + 范式转换模式 | Rust |
| **L3 异步整合层** | Sleep-time Compute：冲突检测、聚合、衰减、预测验证调度 | Rust/Go |
| **L4 语义索引层** | Qdrant 向量存储：语义检索 + 语义距离计算 | Qdrant |
| **L5 知识图谱层** | FalkorDB 图存储：Claim 节点 + 关系 + 激活扩散 | FalkorDB |
| **L6 查询层** | MCP Server：双路径查询(缓存+完整推理)、权限过滤、响应生成 | Rust + MCP |
| **L7 外部知识层** | 订阅管理：RSS/API/爬虫/搜索订阅，文档自动分类 | Go |
| **L8 联邦层** | 多实例联邦：Insight 共享 + 群体智慧四条件检查 | Rust |
| **L9 摄入管道层** | 文档解析：PDF/Word/Markdown → 文本 → EpistemicClaim | Rust |

## 模块依赖关系

```
L1 (持久化层) ──────────┬────────────────────┬────────────────────→ L9 (摄入管道)
                         │                    │
                         ▼                    ▼
L2 (进化引擎) ─────────► L3 (异步整合) ◄───────┘
                              ▲
                              │
         L4 (语义索引) ◄───────┤
              ▲               │
              │               ▼
         L5 (知识图谱) ◄──────┘
              ▲
              │
L6 (MCP Server) ──────────► L7 (外部知识) ──────────► L8 (联邦)
```

## Crate 结构

| Crate | 对应层级 | 状态 |
|-------|---------|------|
| `cogkos-core` | 核心模型 | ✅ |
| `cogkos-store` | L1-L5 存储层 | ✅ |
| `cogkos-mcp` | L6 查询层 | ⚠️ |
| `cogkos-ingest` | L9 摄入管道 | ⚠️ |
| `cogkos-sleep` | L3/L7 异步任务 | ⚠️ |

## 技术栈清单

- **语言**：Rust (核心), Go (订阅调度)
- **数据库**：PostgreSQL, FalkorDB, Qdrant, TimescaleDB
- **对象存储**：S3 (MinIO 本地开发)
- **消息队列**：NATS JetStream (V2+)
- **缓存**：Redis/Dragonfly (V2+)
- **MCP**：Model Context Protocol 2026
- **部署**：Docker/K8s

## 规模化路径

| 规模 | Agent 数量 | 架构 |
|------|-----------|------|
| V1 | ~10 万 | 单实例 |
| V2 | ~100 万 | 分布式缓存 + 多副本 |
| V3 | ~1000 万 | 多区域分片 + 边缘缓存 |
