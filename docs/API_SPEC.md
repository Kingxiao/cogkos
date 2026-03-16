# MCP API 规范

## 协议基础

CogKOS MCP Server 基于 **rmcp SDK 的 MCP 标准协议**。

### 传输方式

| 方式 | 适用场景 | 说明 |
|------|----------|------|
| **stdio** | 本地 Agent（同机部署） | 通过 stdin/stdout 通信，低延迟 |
| **Streamable HTTP** | 远程 Agent | rmcp 提供的流式 HTTP 传输 |
| **HTTP** | REST API 调用 | 标准 HTTP/JSON 接口 |

> rmcp 使用 `#[tool]`/`#[tool_router]`/`#[tool_handler]` 宏定义工具，参数通过 `Parameters<T>` + `schemars::JsonSchema` 自动生成 schema。

### 鉴权方式

所有请求必须携带以下 Header：

```
X-API-Key: <api_key>
X-Tenant-ID: <tenant_id>
```

服务端校验 API Key 后，从 `api_keys` 表获取 `tenant_id` 和 `permissions`，注入全部后续查询。

**权限级别：**
- `read` - 查询知识库
- `write` - 提交经验和文档
- `delete` - 删除知识（需要额外确认）
- `admin` - 管理操作

---

## 工具 1: query_knowledge

查询知识库，返回结构化决策包。

### 功能描述

该工具执行以下操作：
1. 向量相似度搜索（pgvector）
2. 图谱关系扩散（FalkorDB）
3. 置信度排序和过滤
4. 冲突检测
5. 预测生成（可选）
6. 知识空洞识别（可选）

### 请求

```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "query_knowledge",
    "arguments": {
      "query": "竞品X对中小企业的适用性",
      "context": {
        "domain": "竞品分析",
        "urgency": "normal",
        "max_results": 10
      },
      "include_predictions": true,
      "include_conflicts": true,
      "include_gaps": true
    }
  },
  "id": "req-001"
}
```

### 参数说明

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `query` | string | ✅ | - | 自然语言查询语句 |
| `context.domain` | string | ❌ | null | 限定查询领域，提高相关性 |
| `context.urgency` | enum | ❌ | `"normal"` | `"low"` / `"normal"` / `"high"`（高紧急度跳过缓存） |
| `context.max_results` | int | ❌ | `10` | 最大返回条数（1-100） |
| `include_predictions` | bool | ❌ | `true` | 是否基于已有知识生成预测 |
| `include_conflicts` | bool | ❌ | `true` | 是否返回知识冲突 |
| `include_gaps` | bool | ❌ | `true` | 是否识别知识空洞 |

### 响应成功

```json
{
  "jsonrpc": "2.0",
  "result": {
    "content": [
      {
        "type": "text",
        "text": "{...McpQueryResponse JSON...}"
      }
    ]
  },
  "id": "req-001"
}
```

**McpQueryResponse 完整结构：**

```json
{
  "best_belief": {
    "claim_id": "550e8400-e29b-41d4-a716-446655440000",
    "content": "竞品X中小企业满意度低于行业均值",
    "confidence": 0.78,
    "based_on": 5,
    "consolidation_stage": "Consolidated",
    "claim_ids": [
      "550e8400-e29b-41d4-a716-446655440001",
      "550e8400-e29b-41d4-a716-446655440002"
    ]
  },
  "related_by_graph": [
    {
      "claim_id": "550e8400-e29b-41d4-a716-446655440003",
      "content": "竞品X的售后响应慢于同类",
      "relation_type": "CAUSES",
      "activation": 0.72
    },
    {
      "claim_id": "550e8400-e29b-41d4-a716-446655440004",
      "content": "竞品X定价策略偏向大企业",
      "relation_type": "RELATED",
      "activation": 0.65
    }
  ],
  "conflicts": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440005",
      "claim_a_summary": "竞品X价格偏高",
      "claim_b_summary": "竞品X价格有竞争力",
      "conflict_type": "SourceDisagreement",
      "severity": 0.6,
      "detected_at": "2026-03-07T12:00:00Z"
    }
  ],
  "prediction": {
    "content": "推荐竞品X给中小企业的风险较高，建议提供详细部署成本分析",
    "confidence": 0.72,
    "method": "LlmBeliefContext",
    "based_on_claims": [
      "550e8400-e29b-41d4-a716-446655440000",
      "550e8400-e29b-41d4-a716-446655440003"
    ]
  },
  "knowledge_gaps": [
    "缺少竞品X 2026版本数据",
    "中小企业具体需求场景数据不足"
  ],
  "freshness": {
    "newest_source": "2026-02-10T00:00:00Z",
    "oldest_source": "2025-08-15T00:00:00Z",
    "staleness_warning": false
  },
  "cache_status": "miss",
  "query_hash": 12345678901234567890
}
```

### 响应字段说明

| 字段 | 类型 | 说明 |
|------|------|------|
| `best_belief` | object | 最可信的知识信念，包含内容、置信度、来源数量 |
| `related_by_graph` | array | 通过图谱关系发现的相关知识 |
| `conflicts` | array | 检测到的知识冲突 |
| `prediction` | object/null | 基于知识生成的预测 |
| `knowledge_gaps` | array | 识别的知识空洞 |
| `freshness` | object | 数据新鲜度信息 |
| `cache_status` | enum | `"hit"` / `"miss"` / `"stale"` |
| `query_hash` | u64 | 查询哈希，用于后续反馈 |

### 副作用

- 命中的 EpistemicClaim 的 `activation_weight` 原子更新（采样概率由配置决定）
- `access_count++`, `last_accessed = now()`
- 查询结果写入缓存（如果缓存未命中）

---

## 工具 2: submit_experience

Agent 推送经验/观察，作为 Assertion 写入 CogKOS。

### 功能描述

该工具将经验数据写入系统：
1. 创建 EpistemicClaim（初始阶段为 FastTrack）
2. 生成嵌入向量并写入 pgvector
3. 计算与已有知识的语义距离
4. 检测潜在冲突
5. 触发 Sleep-time 聚合任务

### 请求

```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "submit_experience",
    "arguments": {
      "content": "客户A对竞品X的反馈：价格高于预期但功能符合需求",
      "node_type": "Event",
      "confidence": 0.7,
      "source": {
        "type": "agent",
        "agent_id": "forge-001",
        "model": "claude-3.7"
      },
      "valid_from": "2026-03-07T00:00:00Z",
      "valid_to": "2027-03-07T00:00:00Z",
      "tags": ["竞品X", "客户反馈", "定价"],
      "related_to": ["550e8400-e29b-41d4-a716-446655440000"]
    }
  },
  "id": "req-002"
}
```

### 参数说明

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `content` | string | ✅ | - | 知识内容（建议 50-500 字） |
| `node_type` | enum | ✅ | - | `"Entity"` / `"Relation"` / `"Event"` / `"Attribute"` / `"Prediction"` |
| `confidence` | float | ❌ | `0.5` | 置信度 0.0-1.0 |
| `source.type` | enum | ✅ | - | `"human"` / `"agent"` / `"external"` |
| `source.agent_id` | string | 条件 | - | source.type = `"agent"` 时必填 |
| `source.model` | string | 条件 | - | source.type = `"agent"` 时建议填写 |
| `source.user_id` | string | 条件 | - | source.type = `"human"` 时必填 |
| `valid_from` | datetime | ❌ | `now()` | 知识生效开始时间（ISO 8601） |
| `valid_to` | datetime | ❌ | `null` | 知识生效结束时间（可选） |
| `tags` | string[] | ❌ | `[]` | 标签（辅助分类和检索） |
| `related_to` | uuid[] | ❌ | `[]` | 关联已有 Claim 的 ID |

### 响应成功

```json
{
  "jsonrpc": "2.0",
  "result": {
    "content": [
      {
        "type": "text",
        "text": "{\"claim_id\": \"550e8400-e29b-41d4-a716-446655440006\", \"status\": \"accepted\", \"conflicts_detected\": 1, \"novelty_score\": 0.65}"
      }
    ]
  },
  "id": "req-002"
}
```

**解析后的响应结构：**

```json
{
  "claim_id": "550e8400-e29b-41d4-a716-446655440006",
  "status": "accepted",
  "conflicts_detected": 1,
  "novelty_score": 0.65,
  "estimated_consolidation_time": "24h"
}
```

### 状态说明

| 状态 | 说明 |
|------|------|
| `"accepted"` | 成功接收，进入 FastTrack 阶段 |
| `"pending_review"` | 高冲突风险，等待人工审核 |
| `"rejected"` | 被拒绝（重复或无效内容） |

### 副作用

- 创建 EpistemicClaim（FastTrack 阶段）
- 向量化并写入 pgvector
- 计算语义距离：
  - 距离 > 阈值 → "新信息"，触发 Sleep-time 聚合
  - 距离 < 阈值 → "确认"，增强已有知识置信度
- 冲突检测 → 矛盾则创建 ConflictRecord
- 相关查询缓存失效

---

## 工具 3: submit_feedback

对之前查询结果的成功/失败反馈。

### 功能描述

通过反馈机制实现强化学习：
1. 更新查询缓存的置信度
2. 记录预测误差
3. 触发异常信号检测
4. 为进化引擎提供训练信号

### 请求

```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "submit_feedback",
    "arguments": {
      "query_hash": 12345678901234567890,
      "success": false,
      "note": "推荐的方案客户不接受，价格因素被低估",
      "improvement_suggestion": "建议增加成本效益分析"
    }
  },
  "id": "req-003"
}
```

### 参数说明

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `query_hash` | u64 | ✅ | - | 原查询响应中的 `query_hash` |
| `success` | bool | ✅ | - | 决策是否成功 |
| `note` | string | ❌ | `""` | 反馈说明 |
| `improvement_suggestion` | string | ❌ | `""` | 改进建议 |

### 响应成功

```json
{
  "jsonrpc": "2.0",
  "result": {
    "content": [
      {
        "type": "text",
        "text": "{\"status\": \"recorded\", \"cache_adjusted\": true, \"anomaly_score\": 0.3}"
      }
    ]
  },
  "id": "req-003"
}
```

### 副作用

- 查询缓存：`success_count` 或 `confidence` 调整
- Sleep-time：相关知识的 `prediction_error` 更新
- 进化引擎：异常信号检测（累计失败率）

---

## 工具 4: report_gap

Agent 主动报告发现的知识空洞。

### 请求

```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "report_gap",
    "arguments": {
      "domain": "竞品分析",
      "description": "缺少竞品X 2026年第一季度的更新数据",
      "priority": "high",
      "suggested_sources": ["官方文档", "行业报告"]
    }
  },
  "id": "req-004"
}
```

### 参数说明

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `domain` | string | ✅ | - | 知识领域 |
| `description` | string | ✅ | - | 空洞描述 |
| `priority` | enum | ❌ | `"medium"` | `"low"` / `"medium"` / `"high"` |
| `suggested_sources` | string[] | ❌ | `[]` | 建议的信息来源 |

### 响应

```json
{
  "jsonrpc": "2.0",
  "result": {
    "content": [
      {
        "type": "text",
        "text": "{\"gap_id\": \"550e8400-e29b-41d4-a716-446655440007\", \"status\": \"recorded\", \"estimated_fill_time\": \"48h\"}"
      }
    ]
  },
  "id": "req-004"
}
```

---

## 工具 5: get_meta_directory

查询元知识目录（跨实例路由信息）。

### 请求

```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "get_meta_directory",
    "arguments": {
      "query_domain": "供应链优化",
      "min_expertise_score": 0.8
    }
  },
  "id": "req-005"
}
```

### 参数说明

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `query_domain` | string | ✅ | - | 查询领域 |
| `min_expertise_score` | float | ❌ | `0.5` | 最小专业度分数 |

### 响应

```json
{
  "jsonrpc": "2.0",
  "result": {
    "content": [
      {
        "type": "text",
        "text": "{\"entries\": [{\"instance_id\": \"mfg-001\", \"domains\": [\"供应链\", \"MES\"], \"expertise_score\": 0.92, \"endpoint\": \"https://mfg-001.cogkos.cloud/mcp\"}]}"
      }
    ]
  },
  "id": "req-005"
}
```

---

## 工具 6: upload_document

上传文档到 CogKOS（触发摄入管道）。

### 功能描述

文档摄入流程：
1. 文件存入 S3 `/raw/`
2. 创建 File 类型 EpistemicClaim
3. 触发异步解析管道
4. 粗分类 → 全文索引 → 知识提取
5. 生成文本块和嵌入向量

### 请求

```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "upload_document",
    "arguments": {
      "filename": "XX公司2025年度报告.pdf",
      "content_base64": "JVBERi0xLjQKJeLjz9MKMyAwIG9iago8PC9UeXBlL1BhZ2UvUGFyZW50IDIgMCBS...",
      "source": {
        "type": "human",
        "user_id": "admin-001"
      },
      "tags": ["年报", "XX公司", "2025"],
      "auto_process": true
    }
  },
  "id": "req-006"
}
```

### 参数说明

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `filename` | string | ✅ | - | 文件名（含扩展名） |
| `content_base64` | string | ✅ | - | Base64 编码的文件内容 |
| `source` | object | ✅ | - | 来源信息 |
| `tags` | string[] | ❌ | `[]` | 辅助标签 |
| `auto_process` | bool | ❌ | `true` | 是否自动触发解析 |

### 支持的文件类型

| 扩展名 | MIME 类型 | 支持状态 |
|--------|-----------|----------|
| `.pdf` | `application/pdf` | ✅ 完整支持 |
| `.docx` | `application/vnd.openxmlformats-officedocument.wordprocessingml.document` | ✅ 完整支持 |
| `.md` | `text/markdown` | ✅ 完整支持 |
| `.txt` | `text/plain` | ✅ 完整支持 |
| `.xlsx` | `application/vnd.openxmlformats-officedocument.spreadsheetml.sheet` | ✅ 表格解析 |
| `.csv` | `text/csv` | ✅ 表格解析 |
| `.pptx` | `application/vnd.openxmlformats-officedocument.presentationml.presentation` | 🚧 开发中 |

### 响应

```json
{
  "jsonrpc": "2.0",
  "result": {
    "content": [
      {
        "type": "text",
        "text": "{\"file_id\": \"550e8400-e29b-41d4-a716-446655440008\", \"status\": \"ingesting\", \"estimated_time\": \"30s\", \"pipeline_id\": \"pipe-abc123\"}"
      }
    ]
  },
  "id": "req-006"
}
```

### 摄入状态查询

```bash
curl -H "X-API-Key: your-api-key" \
     -H "X-Tenant-ID: your-tenant" \
     http://localhost:3000/pipeline/status/pipe-abc123
```

### 副作用

- 文件存入 S3 `/raw/`
- 创建 File 类型 EpistemicClaim
- 触发异步解析管道（粗分类 → 全文索引 → 知识提取）

---

## HTTP REST API

除 MCP JSON-RPC 接口外，CogKOS 还提供 REST API：

### 健康检查

```bash
GET /health

# 响应
{
  "status": "healthy",
  "version": "0.1.0",
  "components": {
    "postgres": "connected",
    "falkordb": "connected",
    "pgvector": "connected",
    "s3": "connected"
  }
}
```

### 查询知识 (REST)

```bash
POST /api/v1/query
Content-Type: application/json
X-API-Key: your-api-key
X-Tenant-ID: your-tenant

{
  "query": "竞品X的评价",
  "max_results": 10
}
```

### 获取 Claim 详情

```bash
GET /api/v1/claims/{claim_id}
X-API-Key: your-api-key
X-Tenant-ID: your-tenant
```

---

## 错误响应格式

### JSON-RPC 错误格式

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32001,
    "message": "权限不足",
    "data": {
      "error_code": "FORBIDDEN",
      "detail": "API Key 无权访问租户 tenant-002 的数据",
      "request_id": "req-001",
      "timestamp": "2026-03-08T06:00:00Z"
    }
  },
  "id": "req-001"
}
```

### HTTP 错误格式

```json
{
  "error": {
    "code": "RATE_LIMITED",
    "message": "超出频率限制",
    "detail": "当前限制: 1000 请求/小时，已用: 999",
    "retry_after": 3600
  }
}
```

---

## 错误码完整列表

### 输入错误 (4xx 范围)

| error_code | JSON-RPC code | HTTP 状态 | 说明 | 处理建议 |
|------------|---------------|-----------|------|----------|
| `INVALID_INPUT` | -32602 | 400 | 参数无效或缺失 | 检查请求参数是否符合 schema |
| `INVALID_JSON` | -32700 | 400 | JSON 解析错误 | 检查请求体格式 |
| `INVALID_METHOD` | -32601 | 400 | 方法不存在 | 检查工具名称拼写 |
| `INVALID_PARAMS` | -32602 | 400 | 参数类型错误 | 检查参数类型 |
| `TENANT_NOT_FOUND` | -32002 | 400 | 租户不存在 | 确认 X-Tenant-ID 正确 |
| `INVALID_API_KEY` | -32004 | 401 | API Key 无效 | 检查 X-API-Key 是否正确 |
| `API_KEY_EXPIRED` | -32005 | 401 | API Key 已过期 | 申请新的 API Key |
| `FORBIDDEN` | -32001 | 403 | 权限不足 | 确认 API Key 有相应权限 |
| `NOT_FOUND` | -32000 | 404 | 知识/文件不存在 | 确认 ID 正确 |
| `RATE_LIMITED` | -32003 | 429 | 超出频率限制 | 降低请求频率或联系扩容 |
| `CONFLICT` | -32006 | 409 | 资源冲突 | 等待或重试 |

### 系统错误 (5xx 范围)

| error_code | JSON-RPC code | HTTP 状态 | 说明 | 处理建议 |
|------------|---------------|-----------|------|----------|
| `DATABASE_ERROR` | -32010 | 500 | PostgreSQL 错误 | 稍后重试或联系运维 |
| `GRAPH_ERROR` | -32011 | 500 | FalkorDB 错误 | 检查图数据库连接 |
| `VECTOR_ERROR` | -32012 | 500 | pgvector 错误 | 检查向量库状态 |
| `STORAGE_ERROR` | -32013 | 500 | S3 存储错误 | 检查对象存储连接 |
| `LLM_ERROR` | -32014 | 500 | LLM 服务错误 | 检查 LLM 配置 |
| `EMBEDDING_ERROR` | -32015 | 500 | 嵌入模型错误 | 检查嵌入服务 |
| `INTERNAL_ERROR` | -32603 | 500 | 内部错误 | 联系技术支持 |
| `SERVICE_UNAVAILABLE` | -32099 | 503 | 服务不可用 | 稍后重试 |

### 业务错误

| error_code | 说明 | 场景 |
|------------|------|------|
| `CLAIM_EXPIRED` | 知识已过期 | 查询的知识已过有效期 |
| `CLAIM_RETRACTED` | 知识已被撤回 | 知识已被标记为撤回 |
| `CONFLICT_UNRESOLVED` | 冲突未解决 | 存在未解决的冲突 |
| `INGESTION_FAILED` | 摄入失败 | 文档解析或处理失败 |
| `PIPELINE_TIMEOUT` | 管道超时 | 异步处理超时 |

---

## 限流策略

### 默认限流配置

| 级别 | 限制 | 时间窗口 |
|------|------|----------|
| 每个 API Key | 1000 | 1 小时 |
| 每个 IP | 500 | 1 小时 |
| 每个租户 | 10000 | 1 小时 |
| 紧急查询 | 无限制 | - |

### 限流响应头

```
X-RateLimit-Limit: 1000
X-RateLimit-Remaining: 999
X-RateLimit-Reset: 1709836800
X-RateLimit-Retry-After: 3600
```

---

## 版本兼容性

| API 版本 | 状态 | 支持终止日期 |
|----------|------|--------------|
| v1 (current) | 稳定 | - |
| v0.9 | 弃用 | 2026-06-01 |

---

## SDK 和工具

### 官方 SDK

| 语言 | 包名 | 安装 |
|------|------|------|
| Python | `cogkos-client` | `pip install cogkos-client` |
| TypeScript | `@cogkos/client` | `npm install @cogkos/client` |
| Rust | `cogkos-client` | `cargo add cogkos-client` |

### 示例代码

**Python:**
```python
from cogkos import CogKOSClient

client = CogKOSClient(
    api_key="your-api-key",
    tenant_id="your-tenant"
)

# 查询知识
result = client.query_knowledge(
    query="竞品分析",
    include_predictions=True
)

# 提交经验
client.submit_experience(
    content="客户反馈...",
    node_type="Event"
)
```

**TypeScript:**
```typescript
import { CogKOSClient } from '@cogkos/client';

const client = new CogKOSClient({
  apiKey: 'your-api-key',
  tenantId: 'your-tenant'
});

const result = await client.queryKnowledge({
  query: '竞品分析',
  includePredictions: true
});
```

---

## 更新日志

### v0.1.0 (2026-03-08)

- 初始版本发布
- 支持 6 个 MCP 工具
- 完整的多租户支持
- 实现 Sleep-time 计算
- 联邦路由功能

---

## 技术支持

- 文档: https://cogkos.dev/docs
- API 状态: https://status.cogkos.dev
- 支持邮箱: support@cogkos.dev
