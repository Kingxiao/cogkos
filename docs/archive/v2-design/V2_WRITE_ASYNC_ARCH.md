# V2 写入异步化架构设计

> Issue: [#112] [P2] V2 架构设计 - 写入异步化
> 目标：支持 100 万 Agent，异步写入
> 技术要求：NATS JetStream 方案、Worker 消费模式、批量写入优化

## 1. 架构概述

### 1.1 设计目标

| 目标 | 指标 |
|------|------|
| 写入吞吐 | 150K QPS（峰值） |
| 写入延迟 P99 | < 200ms（API 响应） |
| 持久化保证 | 至少一次（at-least-once） |
| 水平扩展 | Worker 无状态，可按需扩缩容 |

### 1.2 整体架构

```
┌─ Agent ─────────────────────┐      ┌─ CogKOS Gateway (Rust) ───────────────┐
│                             │      │                                        │
│  submit_experience()       │      │  ┌─ 校验层 (Validation)                 │
│  submit_feedback()          │ ──→  │  │   - Schema 校验                        │
│  query_knowledge() → 写    │      │  │   - 权限预检                           │
│                             │      │  │   - 元数据补充                        │
└─────────────────────────────┘      │  └────────────┬─────────────────────────┘
                                      │               │
                                      │               ▼
                                      │  ┌─ NATS JetStream Producer ──────────┐
                                      │  │                                    │
                                      │  │   subject: cogkos.write.{tid}     │
                                      │  │   subject: cogkos.feedback.{tid}  │
                                      │  │   subject: cogkos.activation.{tid}│
                                      │  └────────────┬─────────────────────────┘
                                      │               │
                                      └───────────────┼────────────────────────────────┐
                                                      │                                │
                                                      ▼                                │
                                      ┌──────────────────────────────────────────────┐
                                      │            NATS JetStream Cluster           │
                                      │                   (3 节点)                   │
                                      │                                              │
                                      │   Stream: COGKOS_WRITE                      │
                                      │   ├── subject: cogkos.write.*                │
                                      │   ├── subject: cogkos.feedback.*            │
                                      │   ├── subject: cogkos.activation.*          │
                                      │   ├── retention: interest                   │
                                      │   └── storage: file                         │
                                      └────────────────────┬───────────────────────────┘
                                                          │
                        ┌─────────────────────────────────┼─────────────────────────────────┐
                        │                                 │                                 │
                        ▼                                 ▼                                 ▼
            ┌───────────────────────┐        ┌───────────────────────┐        ┌───────────────────────┐
            │   Experience Worker   │        │   Feedback Worker    │        │  Activation Worker    │
            │                       │        │                       │        │                       │
            │  subject:             │        │  subject:            │        │  subject:             │
            │  cogkos.write.{tid}   │        │  cogkos.feedback.{tid}│       │  cogkos.activation.{tid}
            │                       │        │                       │        │                       │
            │  → PostgreSQL         │        │  → Dragonfly (Cache) │        │  → PostgreSQL (批量)   │
            │  → FalkorDB           │        │  → TimescaleDB       │        │  → Dragonfly (缓存)    │
            │  → Qdrant             │        │                       │        │                       │
            └───────────────────────┘        └───────────────────────┘        └───────────────────────┘
```

---

## 2. NATS JetStream 方案

### 2.1 Stream 配置

```yaml
Stream: COGKOS_WRITE
Storage: File  # vs Memory（权衡：持久化 vs 性能）
Retention: Interest  # 消息被所有消费者消费后才删除
Replication: 3  # 3 副本容错
Subject:
  - cogkos.write.*        # 经验写入
  - cogkos.feedback.*     # 反馈写入
  - cogkos.activation.*   # 激活权重更新

# Stream 级别配置
MaxBytes: 100GB           # 单 Stream 最大存储
MaxAge: 7d                # 消息最大保留时间
MaxMsgSize: 1MB           # 单条消息最大
```

### 2.2 Subject 命名规范

```
cogkos.write.{tenant_id}        # 经验写入
cogkos.feedback.{tenant_id}     # Agent 反馈
cogkos.activation.{tenant_id}   # 激活权重更新（高吞吐）
```

### 2.3 消息格式

```rust
// 通用消息封装
struct WriteMessage {
    // Header
    tenant_id: Uuid,
    message_type: MessageType,  // Experience, Feedback, Activation
    correlation_id: Uuid,        // 用于追踪
    timestamp: DateTime<Utc>,
    
    // Payload (JSON/MessagePack)
    payload: Vec<u8>,
    
    // 元数据
    priority: u8,       // 0=低, 1=普通, 2=高
    retry_count: u8,    // 重试次数
}

enum MessageType {
    Experience(ExperiencePayload),
    Feedback(FeedbackPayload),
    Activation(ActivationPayload),
}
```

### 2.4 消息持久化策略

| 消息类型 | 持久化级别 | 理由 |
|---------|-----------|------|
| Experience | **强持久化** | 核心知识资产，不能丢 |
| Feedback | **至少一次** | 可重放，允许少量重复 |
| Activation | **最多一次** | 采样更新，允许丢失 |

---

## 3. Worker 消费模式

### 3.1 Worker 类型

| Worker | Subject | 消费速率 | 写入目标 |
|--------|---------|---------|---------|
| Experience Worker | `cogkos.write.*` | 10K/s | PostgreSQL + FalkorDB + Qdrant |
| Feedback Worker | `cogkos.feedback.*` | 20K/s | Dragonfly + TimescaleDB |
| Activation Worker | `cogkos.activation.*` | 50K/s | PostgreSQL（批量） |

### 3.2 Worker 架构

```rust
// 通用 Worker 框架
struct Worker<C: Consumer, S: Storage> {
    consumer: C,           // NATS 消费者
    storage: S,           // 存储后端
    batch_processor: BatchProcessor,
    metrics: WorkerMetrics,
}

impl<C, S> Worker<C, S>
where
    C: Consumer,
    S: Storage,
{
    async fn run(&self, shutdown: Shutdown) {
        // 1. 获取消息（拉取模式，避免推送风暴）
        let messages = self.consumer.fetch(100, 5_000).await;
        
        // 2. 按类型分组
        let batches = self.group_by_type(messages);
        
        // 3. 批量处理
        for (msg_type, msgs) in batches {
            self.process_batch(msg_type, msgs).await;
        }
        
        // 4. 上报指标
        self.metrics.report().await;
    }
    
    async fn process_batch(&self, msg_type: MessageType, messages: Vec<Msg>) {
        match msg_type {
            MessageType::Experience => {
                // 并发写入多个存储
                tokio::join!(
                    self.write_to_postgres(messages),
                    self.write_to_graph(messages),
                    self.write_to_vector(messages),
                );
            }
            MessageType::Feedback => {
                self.write_feedback(messages).await;
            }
            MessageType::Activation => {
                // 批量聚合后写入
                self.batch_processor.add(messages);
                if self.batch_processor.is_ready() {
                    self.flush_activation_batch().await;
                }
            }
        }
    }
}
```

### 3.3 消费确认机制

```rust
// 消息确认策略
enum AckPolicy {
    // 1. 收到即确认（快速但可能丢消息）
    None,
    
    // 2. 异步确认（写入队列后即确认，真正写入失败则重放）
    Instant,
    
    // 3. 全部完成确认（延迟高，但保证不丢）
    AllDone,
}

// 推荐：Experience 用 AllDone，Feedback/Activation 用 Instant
```

### 3.4 Worker 弹性扩展

```yaml
# K8s HPA 配置示例
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: experience-worker
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: experience-worker
  minReplicas: 3
  maxReplicas: 30
  metrics:
    - type: External
      external:
        metric:
          name: nats_consumer_pending_bytes
          selector:
            matchLabels:
              subject: cogkos.write.*
        target:
          type: AverageValue
          averageValue: "10Mi"
```

---

## 4. 批量写入优化

### 4.1 批量策略

| 操作 | 批量大小 | 超时 | 策略 |
|------|---------|------|------|
| PostgreSQL 写入 | 500 条 | 100ms | 满了或超时 flush |
| FalkorDB 写入 | 100 条 | 50ms | 实时写入，图事务 |
| Qdrant 向量 | 1000 条 | 200ms | 满了或超时 flush |
| Activation 更新 | 1000 条 | 500ms | 采样聚合后批量写 |

### 4.2 Experience 批量写入

```rust
struct ExperienceBatch {
    claims: Vec<EpistemicClaim>,
    tenant_id: Uuid,
    created_at: Instant,
}

impl ExperienceBatch {
    fn should_flush(&self) -> bool {
        self.claims.len() >= 500 
            || self.created_at.elapsed() > Duration::from_millis(100)
    }
    
    async fn flush(&mut self, pool: &PgPool) -> Result<usize> {
        // 1. PostgreSQL 批量插入
        let mut tx = pool.begin().await?;
        let inserted = insert_claims(&mut tx, &self.claims).await?;
        tx.commit().await?;
        
        // 2. FalkorDB 批量写入（异步）
        tokio::spawn(async move {
            let mut graph = connect_graph().await?;
            graph.execute_batch(&build_cypher(&self.claims)).await
        });
        
        // 3. Qdrant 批量向量写入
        tokio::spawn(async move {
            qdrant_client.upsert_points(&build_points(&self.claims)).await
        });
        
        self.claims.clear();
        Ok(inserted)
    }
}
```

### 4.3 Activation 批量聚合

```rust
// Activation 采样 + 聚合
struct ActivationAggregator {
    // Key: (claim_id, agent_id)
    // Value: 累加的 activation_delta
    pending: DashMap<(Uuid, Uuid), f64>,
    flush_interval: Duration,
}

impl ActivationAggregator {
    async fn add(&self, updates: Vec<ActivationUpdate>) {
        for update in updates {
            let counter = self.pending.entry(update.key()).or_insert(0.0);
            *counter += update.delta;
        }
    }
    
    async fn flush(&self, pool: &PgPool) -> Result<()> {
        // 按 claim_id 分组聚合
        let grouped: HashMap<Uuid, Vec<(Uuid, f64)>> = self.pending
            .iter()
            .group_by(|k| k.key().0)
            .into_iter()
            .map(|(k, v)| (k, v.map(|(k, v)| (k.1, *v)).collect()))
            .collect();
        
        // 批量更新
        for (claim_id, deltas) in grouped {
            let query = format!(
                "UPDATE epistemic_claims 
                 SET activation_weight = activation_weight + weight_table.delta
                 FROM (VALUES {}) AS weight_table(agent_id, delta)
                 WHERE id = $1",
                build_values_clause(&deltas)
            );
            pool.execute(&query, claim_id).await?;
        }
        
        self.pending.clear();
        Ok(())
    }
}
```

### 4.4 写入流程图

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           Gateway (同步入口)                                  │
│                                                                             │
│  submit_experience(claims[])                                                │
│       │                                                                     │
│       ├── 1. 校验 Schema                                                    │
│       ├── 2. 补充元数据 (source_id, timestamp, ...)                         │
│       ├── 3. 生成 correlation_id                                           │
│       └── 4. 发送到 NATS (Fire-and-Forget) ──→ P99 < 10ms                  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         NATS JetStream                                        │
│                                                                             │
│   ┌─ 持久化 ───────────────────────────────────────────────────────────┐  │
│   │  message { tenant_id, correlation_id, payload, priority }          │  │
│   └───────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                    ┌───────────────┼───────────────┐
                    │               │               │
                    ▼               ▼               ▼
            ┌───────────┐   ┌───────────┐   ┌───────────┐
            │Experience │   │ Feedback  │   │Activation │
            │ Worker    │   │ Worker    │   │ Worker    │
            │ (x10)     │   │ (x5)      │   │ (x10)     │
            └─────┬─────┘   └─────┬─────┘   └─────┬─────┘
                  │               │               │
                  ▼               ▼               ▼
          ┌───────────┐   ┌───────────┐   ┌───────────┐
          │ Postgres  │   │ Dragonfly │   │ Postgres  │
          │ FalkorDB  │   │ Timescale │   │ (Batch)   │
          │ Qdrant    │   │           │   └───────────┘
          └───────────┘   └───────────┘
```

---

## 5. 容错与恢复

### 5.1 失败处理策略

| 失败类型 | 处理策略 | 重试次数 |
|---------|---------|---------|
| 网络超时 | 指数退避重试 | 3 次 |
| 存储写入失败 | 消息保留，重新消费 | 10 次 |
| 校验失败 | 记录错误日志，跳过 | 0 次 |
| Worker 崩溃 | NATS Redelivery | ∞（直到成功或超时） |

### 5.2 死信队列 (DLQ)

```rust
// 配置 DLQ 处理
let stream_config = StreamConfig {
    name: "COGKOS_WRITE".into(),
    subjects: vec!["cogkos.*".into()],
    max_deliver: 3,  // 最多投递 3 次
    ack_policy: AckPolicy::Explicit,
    // 3 次失败后进入 DLQ
    deliver_subject: Some("COGKOS_WRITE_DLQ".into()),
};
```

### 5.3 监控告警

| 指标 | 告警阈值 | 动作 |
|------|---------|------|
| 消息堆积 | > 100K | 扩容 Worker |
| 消费延迟 P99 | > 5s | 告警 + 扩容 |
| 写入失败率 | > 1% | 告警 + 排查 |
| 重试次数 | > 5 | 记录 + 人工介入 |

---

## 6. 性能基准

### 6.1 目标指标

| 指标 | V1 (当前) | V2 (目标) |
|------|----------|----------|
| 写入吞吐 | 5K/s | 150K/s |
| API 响应 P99 | 50ms | < 200ms |
| 写入持久化延迟 | 同步 | < 100ms |
| 扩展性 | 单实例 | 水平扩展 |

### 6.2 成本估算

| 组件 | 规格 | 数量 | 月成本 |
|------|------|------|--------|
| NATS JetStream | 8C16G | 3 台 | ~¥7K |
| Experience Worker | 8C16G | 10 台 | ~¥15K |
| Feedback Worker | 8C16G | 5 台 | ~¥7.5K |
| Activation Worker | 8C16G | 10 台 | ~¥15K |
| **总计** | | **28 台** | **~¥44.5K/月** |

---

## 7. 实施计划

### Phase 1: 基础架构 (1 周)

- [ ] 部署 NATS JetStream 集群
- [ ] 实现 Gateway NATS Producer
- [ ] 实现 Experience Worker 基础版

### Phase 2: 批量优化 (1 周)

- [ ] 实现批量写入逻辑
- [ ] 实现 Feedback Worker
- [ ] 实现 Activation Worker

### Phase 3: 弹性与监控 (1 周)

- [ ] K8s HPA 配置
- [ ] 监控告警接入
- [ ] DLQ 处理流程

---

## 8. 附录

### A. 技术选型理由

| 选型 | 理由 |
|------|------|
| NATS JetStream | 轻量、Rust 原生客户端、低延迟、高吞吐、持久化保证 |
| 拉取消费 | 避免推送模式导致的消费者过载 |
| 批量写入 | PostgreSQL 批量插入比单条快 10-50x |

### B. 替代方案对比

| 方案 | 优点 | 缺点 | 适用场景 |
|------|------|------|---------|
| **NATS JetStream** | 轻量、低延迟 | 无事务消息 | 高吞吐异步写入 ✓ |
| Kafka | 生态丰富 | 重、延迟高 | 大数据管道 |
| Redis Streams | 简单 | 持久化弱 | 小规模 |

### C. 相关配置

```yaml
# docker-compose.yml 示例
nats:
  image: nats:2.10
  command: [
    "--js", 
    "--cluster", "nats-cluster",
    "--storage=file",
    "--max_store=100GB",
    "--max_payload=1MB",
    "--max_connections=10000"
  ]
```
