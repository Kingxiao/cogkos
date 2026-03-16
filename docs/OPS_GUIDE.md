# CogKOS 运维手册

**版本**: 1.0
**更新时间**: 2026-03-11

---

## 目录

1. [部署配置](#部署配置)
2. [环境变量](#环境变量)
3. [监控配置](#监控配置)
4. [日志配置](#日志配置)
5. [备份恢复](#备份恢复)
6. [故障排查](#故障排查)
7. [性能调优](#性能调优)

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

---

## 扩展阅读

- [架构设计](./ARCHITECTURE.md)
- [API 规范](./API_SPEC.md)
- [数据模型](./DATA_MODELS.md)
- [开发指南](./PROJECT_SETUP.md)
