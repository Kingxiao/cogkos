# CogKOS 运维手册

**版本**: 1.1
**更新时间**: 2026-03-24

---

## 目录

1. [部署配置](#部署配置)
2. [环境变量](#环境变量)
3. [监控配置](#监控配置)
4. [日志配置](#日志配置)
5. [BGE-M3 Embedding 部署](#bge-m3-embedding-部署)
6. [备份恢复](#备份恢复)
7. [批量导入](#批量导入)
8. [审计日志](#审计日志)
9. [高可用部署](#高可用部署)
10. [故障排查](#故障排查)
11. [性能调优](#性能调优)

---

## 部署配置

### Docker Compose 部署

```yaml
version: '3.8'
services:
  cogkos:
    image: cogkos:latest
    ports:
      - "3000:3000"
    environment:
      - DATABASE_URL=postgres://cogkos:password@postgres:5432/cogkos
      - FALKORDB_URL=redis://falkordb:6379
    depends_on:
      - postgres
      - falkordb
    volumes:
      - ./config:/app/config
```

### Kubernetes 部署

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: cogkos
spec:
  replicas: 3
  selector:
    matchLabels:
      app: cogkos
  template:
    spec:
      containers:
      - name: cogkos
       :latest
        ports image: cogkos:
        - containerPort: 3000
        env:
        - name: DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: cogkos-secrets
              key: database-url
```

---

## 环境变量

### 必需变量

| 变量名 | 说明 | 默认值 |
|--------|------|--------|
| `DATABASE_URL` | PostgreSQL 连接地址 | postgres://cogkos:cogkos_dev@localhost:5432/cogkos |
| `FALKORDB_URL` | FalkorDB 服务地址 | redis://localhost:6379 |

### 可选变量

| 变量名 | 说明 | 默认值 |
|--------|------|--------|
| `MCP_HOST` | MCP 服务器监听地址 | 0.0.0.0 |
| `MCP_PORT` | MCP 服务器监听端口 | 3000 |
| `S3_ENDPOINT` | S3 兼容存储端点 | - |
| `S3_REGION` | S3 区域 | us-east-1 |
| `S3_BUCKET` | S3 存储桶 | cogkos-docs |
| `RUST_LOG` | 日志级别 | info |

---

## 监控配置

### Prometheus 指标

CogKOS 通过 `/metrics` 端点暴露 Prometheus 指标：

```bash
# 获取指标
curl http://localhost:3000/metrics
```

### 可用指标

| 指标名称 | 类型 | 说明 |
|----------|------|------|
| `query_latency` | Histogram | 查询延迟 |
| `cache_hits_total` | Counter | 缓存命中次数 |
| `claims_stored` | Gauge | 存储的 Claim 数量 |
| `graph_nodes` | Gauge | 图数据库节点数 |

### Grafana 配置

```json
{
  "dashboard": {
    "title": "CogKOS",
    "panels": [
      {
        "title": "Query Latency",
        "targets": [
          {
            "expr": "histogram_quantile(0.95, rate(query_latency_bucket[5m]))"
          }
        ]
      }
    ]
  }
}
```

---

## 日志配置

### 结构化日志

CogKOS 使用 JSON 格式的结构化日志：

```json
{
  "timestamp": "2026-03-11T10:00:00Z",
  "level": "INFO",
  "target": "cogkos_mcp::server",
  "message": "MCP request received",
  "method": "tools/call"
}
```

### 日志级别

通过 `RUST_LOG` 环境变量配置：

```bash
# 最高级别
RUST_LOG=trace

# 调试信息
RUST_LOG=debug

# 信息
RUST_LOG=info

# 警告
RUST_LOG=warn

# 错误
RUST_LOG=error
```

### 日志收集

推荐使用 Loki 进行日志收集：

```yaml
scrape_configs:
  - job_name: cogkos
    static_configs:
      - targets: ['cogkos:3000']
```

---

## BGE-M3 Embedding 部署

CogKOS 支持两种 Embedding 后端：本地 BGE-M3（通过 TEI）和远程 API（302.ai / OpenAI）。

### GPU 模式（推荐）

使用 Hugging Face Text Embeddings Inference (TEI) 部署 BGE-M3：

```bash
# 拉取模型（首次约 2.4GB）
docker run --gpus all -p 8080:80 \
  -v $HOME/.cache/huggingface:/data \
  ghcr.io/huggingface/text-embeddings-inference:latest \
  --model-id BAAI/bge-m3 \
  --max-client-batch-size 128
```

GPU 模式下单请求延迟约 5-15ms，吞吐量 500+ req/s。

### CPU 模式（降级方案）

无 GPU 时使用 CPU profile：

```bash
docker run -p 8080:80 \
  -v $HOME/.cache/huggingface:/data \
  ghcr.io/huggingface/text-embeddings-inference:cpu-latest \
  --model-id BAAI/bge-m3 \
  --max-client-batch-size 32
```

CPU 模式延迟约 50-200ms，适合开发/低流量环境。

### 模型下载和缓存

- 模型文件缓存在 `$HOME/.cache/huggingface/hub/models--BAAI--bge-m3/`
- 首次启动自动下载，后续使用缓存
- 离线部署时可预先下载：`huggingface-cli download BAAI/bge-m3`
- 向量维度：1024d（CogKOS 默认配置）

### 健康检查

```bash
# TEI 健康检查
curl http://localhost:8080/health

# 测试 Embedding 生成
curl -X POST http://localhost:8080/embed \
  -H 'Content-Type: application/json' \
  -d '{"inputs": "test embedding"}'
```

### CogKOS 对接配置

```bash
# .env
EMBEDDING_PROVIDER=local
EMBEDDING_BASE_URL=http://localhost:8080
EMBEDDING_MODEL=BAAI/bge-m3
EMBEDDING_DIMENSION=1024
```

---

## 备份恢复

### PostgreSQL 备份

```bash
# 完整备份
pg_dump -U cogkos -F c cogkos > backup.dump

# 增量备份 (使用 wal-g)
wal-g backup-push /var/lib/postgresql/data
```

### 恢复

```bash
# 恢复完整备份
pg_restore -U cogkos -d cogkos -c backup.dump
```

### FalkorDB 备份

```bash
# 导出图数据
redis-cli -h falkordb SAVE
```

### pgvector 备份

pgvector 数据随 PostgreSQL 一起备份，无需单独操作。

### 自动化备份脚本

项目提供一键备份和恢复脚本：

```bash
# 完整备份（PostgreSQL + FalkorDB + 本地文件）
bash scripts/backup.sh ./backups

# 从备份恢复
bash scripts/restore.sh ./backups/cogkos_backup_20260324_120000.tar.gz
```

### 定时备份（cron）

```bash
# 每天凌晨 3 点备份，保留最近 30 天
0 3 * * * cd /opt/cogkos && bash scripts/backup.sh ./backups && find ./backups -name '*.tar.gz' -mtime +30 -delete
```

建议同时配置远程备份（rsync/rclone 到 S3）防止单点存储故障。

---

## 批量导入

### 使用方法

```bash
# 扫描目录并导入所有支持的文件
python scripts/batch-import.py /path/to/docs/ \
  --url http://localhost:3000/mcp \
  --api-key your-key \
  --tenant-id your-tenant

# 预览模式（不实际导入）
python scripts/batch-import.py /path/to/docs/ --dry-run
```

### 支持的格式

| 类别 | 格式 |
|------|------|
| 文档 | PDF, DOCX, MD, TXT, HTML |
| 数据 | XLSX, CSV, JSON, XML, YAML |
| 图片 | PNG, JPG/JPEG |

### 特性

- 按文件大小排序导入（小文件优先，避免超时阻塞）
- 通过 content hash 去重，重复文件自动跳过
- 超过 50MB 的文件自动跳过
- 失败不中断，最后输出汇总统计
- 支持通过环境变量配置（`COGKOS_URL`, `COGKOS_API_KEY`, `COGKOS_TENANT_ID`）

---

## 审计日志

### 导出方法

```bash
# 导出为 CSV（默认）
bash scripts/export-audit.sh --since 2026-01-01 audit.csv

# 导出为 JSON
bash scripts/export-audit.sh --format json --tenant cogkos-dev audit.json

# 输出到 stdout
bash scripts/export-audit.sh --since 2026-03-01
```

### 数据来源

脚本优先查询 `audit_logs` 表（结构化审计日志），若该表不存在则回退到 `epistemic_claims` + `agent_feedbacks` 的操作记录合并视图。

### 导出字段

| 字段 | 说明 |
|------|------|
| id | 记录唯一 ID |
| timestamp | 操作时间 (ISO 8601) |
| action | 操作类型 |
| category | 分类（Knowledge/Feedback/System） |
| severity | 级别（Info/Warning/Error） |
| tenant_id | 租户 ID |
| actor | 操作者（用户/服务/Agent） |
| target | 操作目标（资源类型:ID） |
| outcome | 结果（Success/Failure） |

### 合规建议

- 审计日志应至少保留 90 天（金融/医疗行业可能要求更长）
- 定期导出并归档到不可变存储（S3 Object Lock / WORM）
- 配合 `audit_logs` 表的 RLS 策略，确保跨租户隔离
- 建议每月生成合规报告并存档

---

## 高可用部署

### 架构概述

CogKOS 本身无状态（所有状态存于 PostgreSQL + FalkorDB），天然支持横向扩展。多实例部署只需负载均衡即可。

### 部署拓扑

```
Production HA Topology:
                    ┌─ Load Balancer ─┐
                    │  (nginx/traefik) │
                    └──┬─────────┬────┘
                       │         │
              ┌────────┤         ├────────┐
              │        │         │        │
         CogKOS-1  CogKOS-2  CogKOS-3
              │        │         │
              └────────┤         ├────────┘
                       │         │
              ┌────────┴─────────┴────────┐
              │   PostgreSQL Primary      │
              │   ↓ streaming replication │
              │   PostgreSQL Replica      │
              └───────────────────────────┘
              ┌───────────────────────────┐
              │   FalkorDB (Sentinel)     │
              └───────────────────────────┘
              ┌───────────────────────────┐
              │   TEI-1  TEI-2 (BGE-M3)  │
              └───────────────────────────┘
```

### CogKOS 实例

- 无状态，可随时启停
- 建议至少 3 实例保证可用性
- 负载均衡器推荐：Traefik（自动服务发现）或 nginx（简单稳定）
- 健康检查端点：`GET /health`，返回各组件状态

### PostgreSQL 高可用

| 方案 | 复杂度 | 适用场景 |
|------|--------|----------|
| Streaming Replication | 低 | 中小规模，手动故障切换 |
| Patroni + etcd | 中 | 生产环境，自动故障切换 |
| Citus | 高 | 大规模分布式，需要水平分片 |

推荐配置：

```yaml
# Patroni 最小配置
scope: cogkos-cluster
namespace: /cogkos
name: pg-node-1
restapi:
  listen: 0.0.0.0:8008
bootstrap:
  dcs:
    postgresql:
      parameters:
        max_connections: 200
        shared_buffers: 2GB
        wal_level: replica
        max_wal_senders: 5
```

pgvector 索引在 Replica 上可用于只读查询（语义搜索分流）。

### FalkorDB 高可用

使用 Redis Sentinel 或 Cluster 模式：

```bash
# Sentinel 模式（推荐，简单可靠）
# sentinel.conf
sentinel monitor cogkos-falkordb falkordb-primary 6379 2
sentinel down-after-milliseconds cogkos-falkordb 5000
sentinel failover-timeout cogkos-falkordb 10000
```

CogKOS 通过 `FALKORDB_URL` 连接，Sentinel 模式下指向 Sentinel 地址即可。

### BGE-M3 TEI 高可用

- 部署多个 TEI 实例，CogKOS 通过 round-robin 负载均衡访问
- GPU 实例和 CPU 实例可混合部署（GPU 优先，CPU 降级）
- 建议至少 2 个实例，避免 Embedding 服务单点故障

```yaml
# docker-compose HA 配置示例
services:
  tei-1:
    image: ghcr.io/huggingface/text-embeddings-inference:latest
    deploy:
      resources:
        reservations:
          devices:
            - capabilities: [gpu]
    ports:
      - "8080:80"
  tei-2:
    image: ghcr.io/huggingface/text-embeddings-inference:latest
    deploy:
      resources:
        reservations:
          devices:
            - capabilities: [gpu]
    ports:
      - "8081:80"
```

### 容量规划

| 组件 | 单实例推荐 | 备注 |
|------|-----------|------|
| CogKOS | 2 CPU / 2GB RAM | 无状态，受限于下游连接数 |
| PostgreSQL | 4 CPU / 8GB RAM / SSD | 主要瓶颈在 pgvector 索引 |
| FalkorDB | 2 CPU / 4GB RAM | 内存受图规模影响 |
| TEI (GPU) | 1 GPU / 4GB VRAM | 单实例即可支撑 500+ req/s |
| TEI (CPU) | 4 CPU / 4GB RAM | 延迟高，仅作降级方案 |

---

## 故障排查

### 服务无法启动

1. 检查依赖服务是否运行：
   ```bash
   docker-compose ps
   ```

2. 检查环境变量配置：
   ```bash
   docker-compose logs cogkos
   ```

3. 验证数据库连接：
   ```bash
   psql $DATABASE_URL -c "SELECT 1"
   ```

### 查询性能下降

1. 检查缓存命中率：
   ```bash
   curl -s http://localhost:3000/metrics | grep cache_hits
   ```

2. 检查 pgvector 索引状态：
   ```bash
   psql $DATABASE_URL -c "SELECT * FROM pg_stat_user_indexes WHERE indexrelname LIKE '%embedding%';"
   ```

3. 查看慢查询日志

### 内存使用过高

1. 检查图数据库连接数：
   ```bash
   redis-cli -h falkordb INFO clients
   ```

2. 调整连接池大小：
   ```yaml
   # config/default.toml
   [database]
   max_connections = 50
   ```

### 数据不一致

1. 重新同步图数据库：
   ```bash
   # 清理缓存
   curl -X POST http://localhost:3000/admin/clear-cache
   ```

2. 重新运行知识整合：
   ```bash
   curl -X POST http://localhost:3000/admin/reindex
   ```

### Embedding 服务不可用

1. 检查 TEI 实例状态：
   ```bash
   curl http://localhost:8080/health
   ```

2. 检查 GPU 驱动：
   ```bash
   nvidia-smi  # 确认 GPU 可用
   ```

3. 降级到远程 API：
   ```bash
   # 切换到 302.ai 远程 Embedding
   export EMBEDDING_PROVIDER=openai
   export EMBEDDING_BASE_URL=https://api.302.ai/v1
   export EMBEDDING_MODEL=text-embedding-3-large
   ```

### MCP 会话超时

1. 检查 MCP 端口是否正常监听：
   ```bash
   ss -tlnp | grep 3000
   ```

2. 验证 MCP 初始化：
   ```bash
   curl -X POST http://localhost:3000/mcp \
     -H 'Content-Type: application/json' \
     -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"debug","version":"0.1"}}}'
   ```

3. 检查日志中的认证错误：
   ```bash
   RUST_LOG=cogkos_mcp=debug ./target/release/cogkos 2>&1 | grep -i auth
   ```

### 日志位置和级别调整

| 部署方式 | 日志位置 |
|----------|---------|
| systemd user service | `journalctl --user -u cogkos` |
| Docker | `docker logs cogkos` |
| 直接运行 | stdout/stderr |

运行时调整日志级别（不需要重启）：

```bash
# 全局 debug
RUST_LOG=debug

# 模块级别控制
RUST_LOG=cogkos_mcp=debug,cogkos_store=info,cogkos_sleep=warn

# 仅看 SQL 查询
RUST_LOG=sqlx=debug
```

---

## 性能调优

### PostgreSQL 优化

```sql
-- 创建索引
CREATE INDEX idx_claims_tenant ON claims(tenant_id);
CREATE INDEX idx_claims_confidence ON claims(confidence);
CREATE INDEX idx_claims_created ON claims(created_at DESC);

-- 分析表
ANALYZE claims;
```

### pgvector 优化

```sql
-- 调整 HNSW 索引参数
ALTER INDEX idx_claims_embedding SET (ef_construction = 128);

-- 查询时调整搜索精度
SET hnsw.ef_search = 100;
```

### 缓存配置

```toml
[cache]
enabled = true
ttl_seconds = 3600
max_entries = 10000
```

### 内存配置

```toml
[memory]
# 图数据库内存限制 (GB)
graph_max_memory = 4

# 向量缓存大小 (GB)
vector_cache_size = 2
```

---

## 健康检查

### 端点检查

```bash
# 检查服务健康状态
curl http://localhost:3000/health

# 响应示例
{"status": "healthy", "components": {"postgres": "up", "pgvector": "up", "falkordb": "up"}}
```

### 组件检查

| 组件 | 检查方法 | 端口 |
|------|----------|------|
| PostgreSQL (含 pgvector) | TCP 连接 | 5432 |
| FalkorDB | Redis PING | 6379 |
| S3 | HEAD Bucket | 9000 |

---

## 安全配置

### API 密钥管理

1. 生成强随机密钥：
   ```bash
   openssl rand -hex 32
   ```

2. 配置到环境变量：
   ```bash
   export API_KEY=your_generated_key
   ```

### 数据加密

CogKOS 支持 AES-256-GCM 加密敏感数据：

```rust
use cogkos_core::encryption::{encrypt, decrypt};

// 加密敏感数据
let encrypted = encrypt("sensitive_value").unwrap();

// 解密数据
let decrypted = decrypt(&encrypted).unwrap();
```

### RBAC 配置

```toml
[security]
# 角色定义
roles = ["admin", "editor", "viewer"]

# API 密钥认证
require_api_key = true
```

### Security Mode

CogKOS 通过 `COGKOS_ENV` 环境变量控制安全级别，一个开关联动所有安全行为。

#### Production Deployment Checklist

设置 `COGKOS_ENV=production` 启用全部安全控制：

- **DEFAULT_MCP_API_KEY 失效** — 生产模式忽略 dev key，使用 `cogkos-admin create-key` 创建 API key
- **CORS 限制** — 仅允许 `CORS_ALLOWED_ORIGINS` 配置的来源（逗号分隔）
- **启动警告** — 检测 DATABASE_URL 缺少 sslmode、FalkorDB 无密码等安全隐患
- **审计日志持久化** — 审计日志写入 PostgreSQL

```bash
# 生产模式启动
COGKOS_ENV=production \
CORS_ALLOWED_ORIGINS=https://app.example.com,https://admin.example.com \
./target/release/cogkos
```

#### Infrastructure Security (handled outside CogKOS)

- **TLS 终止**: nginx/traefik 反向代理
- **FalkorDB 密码**: redis.conf `requirepass`
- **PostgreSQL SSL**: 连接字符串 `sslmode=require`
- **网络隔离**: Docker internal network 或 VPC

---

## 扩展阅读

- [架构设计](./ARCHITECTURE.md)
- [API 规范](./API_SPEC.md)
- [数据模型](./DATA_MODELS.md)
- [开发指南](./PROJECT_SETUP.md)
