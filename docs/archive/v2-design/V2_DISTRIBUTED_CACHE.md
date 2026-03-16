# V2 分布式缓存架构设计

## 概述

本文档定义 CogKOS V2 架构中的分布式缓存方案，目标是将 80% 的查询流量拦截在缓存层，使后端存储降压 5 倍。

## 1. 架构概览

```
Agent → L4 负载均衡
          │
          ▼
   Gateway Cluster (Rust × 10-20 台)
          │
          ├── 命中 → Dragonfly 缓存集群 (3 节点)
          │
          └── 未命中 → 存储层 (读写分离)
```

### 1.1 集群拓扑

| 节点 | 规格 | 数量 | 角色 |
|------|------|------|------|
| Dragonfly-1 | 16C64G | 1 | 主节点 |
| Dragonfly-2 | 16C64G | 1 | 副本节点 |
| Dragonfly-3 | 16C64G | 1 | 副本节点 |

**部署模式**：一主两从，异步复制。

## 2. 缓存策略

### 2.1 缓存内容分类

| 缓存类型 | Key 格式 | TTL | 失效策略 |
|---------|---------|-----|---------|
| **查询结果** | `t:{tenant_id}:q:{query_hash}` | 10-60 分钟（可配置） | 对应知识写入时主动失效 |
| **热门知识** | `t:{tenant_id}:c:{claim_id}` | 5 分钟 | 写入时失效 |
| **激活扩散结果** | `t:{tenant_id}:s:{node_id}` | 30 分钟 | Sleep-time 定期刷新 |

### 2.2 查询结果缓存

```rust
// 缓存键生成
fn cache_key(tenant_id: &Uuid, query: &str) -> String {
    let hash = calculate_hash(query);
    format!("t:{}:q:{:x}", tenant_id, hash)
}

// 缓存值结构
struct CachedQueryResult {
    response: McpQueryResponse,
    confidence: f64,
    hit_count: u64,
    success_count: u64,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    // 用于失效追踪
    dependent_claims: Vec<Uuid>,
}
```

**TTL 配置策略**：
- 高频固定查询（如"公司简介"）：60 分钟
- 中频分析查询（如"竞品对比"）：30 分钟
- 低频探索查询：10 分钟

### 2.3 热门知识缓存

```rust
// 热门知识缓存键
fn hot_claim_key(tenant_id: &Uuid, claim_id: &Uuid) -> String {
    format!("t:{}:c:{}", tenant_id, claim_id)
}

// 缓存值
struct CachedClaim {
    claim: EpistemicClaim,
    access_count: u64,
    last_accessed: DateTime<Utc>,
}
```

### 2.4 激活扩散结果缓存

```rust
// 激活扩散缓存键
fn activation_key(tenant_id: &Uuid, node_id: &Uuid) -> String {
    format!("t:{}:s:{}", tenant_id, node_id)
}

// 缓存值
struct CachedActivationResult {
    activated_nodes: Vec<ActivatedNode>,
    computed_at: DateTime<Utc>,
}
```

## 3. 失效机制

### 3.1 主动失效（Write-Invalidate）

```
写入请求 → 校验 → 写入存储层
              │
              ├── 同步失效缓存键（相关查询结果）
              ├── 同步失效热门知识
              └── 异步通知刷新激活扩散缓存
```

```rust
async fn invalidate_on_write(tenant_id: Uuid, claim: &EpistemicClaim) {
    // 1. 失效相关查询结果
    let query_pattern = format!("t:{}:q:*", tenant_id);
    dragonfly.invalidate_pattern(&query_pattern).await;

    // 2. 失效热门知识
    let claim_key = format!("t:{}:c:{}", tenant_id, claim.id);
    dragonfly.delete(&claim_key).await;

    // 3. 标记激活扩散缓存需要刷新
    let activation_key = format!("t:{}:s:{}", tenant_id, claim.id);
    dragonfly.set_with_flags(
        &activation_key,
        "STALE",
        Expiry::Minutes(1),  // 1分钟后自动过期
        Flag::InvalSync,
    ).await;
}
```

### 3.2 被动失效（TTL + 置信度）

```rust
// 缓存失效条件（任一触发则跳过缓存）：
// 1. confidence < 0.5
// 2. 缓存创建后相关知识有写入更新（通过版本号检测）
// 3. success_rate < 0.4 (success_count / hit_count)
// 4. 缓存超过 TTL
```

### 3.3 失效传播拓扑

```
┌─────────────────────────────────────────────────────────────┐
│                    失效传播策略                              │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  写入 Claim A                                               │
│       │                                                     │
│       ▼                                                     │
│  ┌─────────────┐     直接相关      ┌─────────────────────┐  │
│  │ 查询结果缓存 │ ←────────────── │ q:hash(A相关查询)   │  │
│  │ t:{t}:q:*   │                  └─────────────────────┘  │
│  └─────────────┘                                         │
│       │                                                     │
│       │ 模式匹配失效                                         │
│       ▼                                                     │
│  ┌─────────────┐     热门知识        ┌─────────────────────┐  │
│  │ 热门知识缓存 │ ←────────────── │ c:claim_A            │  │
│  │ t:{t}:c:*   │                  └─────────────────────┘  │
│  └─────────────┘                                         │
│       │                                                     │
│       │ 标记为 STALE                ┌─────────────────────┐  │
│  ┌─────────────┐ ←───────────────→ │ s:node_A (扩散结果)  │  │
│  │ 扩散结果缓存 │  异步刷新         └─────────────────────┘  │
│  │ t:{t}:s:*   │                                          │
│  └─────────────┘                                          │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 4. Dragonfly 集群配置

### 4.1 集群拓扑

```yaml
# docker-compose.yml 片段
services:
  dragonfly:
    image: dragonflydb/dragonfly:vlatest
    command:
      - --port=6379
      - --dbfilename=
      - --maxmemory=48gb
      - --maxmemory-policy=allkeys-lru
      - --replicaof=dragonfly-1 6379  # for replica nodes
    deploy:
      replicas: 3
    volumes:
      - dragonfly-data:/data
```

### 4.2 高可用配置

| 配置项 | 值 | 说明 |
|--------|-----|------|
| `maxmemory` | 48GB | 预留 25% 内存冗余 |
| `maxmemory-policy` | allkeys-lru | 内存不足时淘汰最少访问 |
| `replicaof` | 异步复制 | 主从异步，延迟 < 100ms |
| `timeout` | 300s | 空闲连接超时 |

### 4.3 性能指标目标

| 指标 | 目标值 |
|------|--------|
| 单实例 QPS | 100K+ |
| 集群总 QPS | 300K+ |
| P50 延迟 | < 1ms |
| P99 延迟 | < 5ms |
| 缓存命中率 | > 80% |

## 5. 缓存读写流程

### 5.1 读流程

```
Agent 查询请求
      │
      ▼
┌─────────────────┐
│ 检查查询缓存     │──命中──→ 返回缓存结果（更新 hit_count）
│ t:{t}:q:{hash}  │
└────────┬────────┘
         │未命中
         ▼
┌─────────────────┐
│ 检查热门知识缓存 │──命中──→ 从热门知识构建响应
│ t:{t}:c:{id}    │
└────────┬────────┘
         │未命中
         ▼
    完整推理路径
    (存储层查询)
         │
         ▼
    写入缓存
         │
         ▼
      返回响应
```

### 5.2 写流程

```
Agent 写入请求
      │
      ▼
┌─────────────────┐
│ 权限校验         │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ 写入存储层       │ (PostgreSQL + FalkorDB + Qdrant)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ 主动失效缓存     │
│ (同步)          │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ 通知刷新扩散缓存 │
│ (异步)          │
└─────────────────┘
```

## 6. 监控指标

### 6.1 缓存层指标

| 指标 | 告警阈值 |
|------|---------|
| 缓存命中率 | < 70% |
| 内存使用率 | > 85% |
| 命令执行 P99 | > 10ms |
| 复制延迟 | > 200ms |
| 连接数使用率 | > 80% |

### 6.2 缓存操作指标

```rust
// 需要上报的指标
struct CacheMetrics {
    hits: Counter,
    misses: Counter,
    invalidations: Counter,
    evictions: Counter,
    latency_p50: Histogram,
    latency_p99: Histogram,
}
```

## 7. 与现有架构的集成

### 7.1 修改点

| 位置 | 修改内容 |
|------|---------|
| `crates/mcp/src/query.rs` | 增加缓存查询逻辑 |
| `crates/mcp/src/write.rs` | 增加写入时失效逻辑 |
| `crates/cache/src/lib.rs` | 新增缓存抽象层 |
| `docker-compose.yml` | 增加 Dragonfly 服务 |

### 7.2 配置新增

```yaml
# config/cache.yaml
cache:
  enabled: true
  cluster:
    nodes:
      - host: dragonfly-1
        port: 6379
      - host: dragonfly-2
        port: 6379
      - host: dragonfly-3
        port: 6379
  ttl:
    query_result: 1800  # 30分钟
    hot_claim: 300      # 5分钟
    activation: 1800    # 30分钟
  invalidation:
    sync: true
    async: true
```

## 8. 成本估算

| 组件 | 规格 | 数量 | 月成本 |
|------|------|------|--------|
| Dragonfly | 16C64G | 3 台 | ~¥4.5K |

**相比 V1 的增量成本**：~¥4.5K/月（缓存层）

**收益**：
- 后端存储 QPS 降低 80%
- 存储层成本节省约 50%（可减少存储实例）
- 查询延迟 P50 从 50ms 降至 5ms

## 9. 后续扩展

### V3 缓存增强（1000 万 Agent）

| 改进 | 内容 |
|------|------|
| Edge Cache | 5 节点 Dragonfly 预计算结果 |
| 预热机制 | 启动时加载 Top 1000 热门查询 |
| 分层缓存 | L1 内存 + L2 Dragonfly |

---

**文档版本**：v1.0  
**创建日期**：2026-03-10  
**关联 Issue**：#111  
**状态**：Draft → 需要评审
