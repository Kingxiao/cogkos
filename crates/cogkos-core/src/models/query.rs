use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{ConflictRecord, ConflictType, ConsolidationStage, TenantId};

/// Graph node for knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: Uuid,
    pub content: String,
    pub activation: f64,
}

/// Vector match result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorMatch {
    pub id: Uuid,
    pub score: f64,
}

/// Query filter variants
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum QueryFilter {
    /// Filter by consolidation stage
    Stage { stage: ConsolidationStage },
    /// Filter by confidence range
    Confidence { min: f64, max: f64 },
    /// Filter by tenant
    Tenant { tenant_id: TenantId },
    /// Filter by epistemic status
    Status { status: String },
    /// Filter by node type
    NodeType { node_type: String },
    /// Filter by date range
    DateRange {
        field: String,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
    },
}

/// Query request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    pub query: String,
    pub tenant_id: TenantId,
    pub filters: Vec<QueryFilter>,
    pub limit: u32,
    pub offset: u32,
    #[serde(default)]
    pub include_related: bool,
    #[serde(default)]
    pub include_conflicts: bool,
}

impl QueryRequest {
    /// Create a new query request
    pub fn new(query: impl Into<String>, tenant_id: TenantId) -> Self {
        Self {
            query: query.into(),
            tenant_id,
            filters: Vec::new(),
            limit: 10,
            offset: 0,
            include_related: false,
            include_conflicts: false,
        }
    }

    /// Add a filter
    pub fn with_filter(mut self, filter: QueryFilter) -> Self {
        self.filters.push(filter);
        self
    }

    /// Set limit
    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = limit;
        self
    }
}

/// Query response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    pub claims: Vec<super::EpistemicClaim>,
    pub total_count: usize,
    pub query: String,
}

/// MCP query response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpQueryResponse {
    pub query_hash: u64,
    pub query_context: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_belief: Option<BeliefSummary>,
    #[serde(default)]
    pub related_by_graph: Vec<GraphRelation>,
    #[serde(default)]
    pub conflicts: Vec<ConflictSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prediction: Option<PredictionResult>,
    #[serde(default)]
    pub knowledge_gaps: Vec<String>,
    pub freshness: FreshnessInfo,
    pub cache_status: CacheStatus,
    /// Which cognitive path was used (S6: System 1 fast / System 2 full reasoning)
    #[serde(default)]
    pub cognitive_path: Option<CognitivePath>,
    #[serde(default)]
    pub metadata: QueryMetadata,
}

/// Metadata for query performance and stats
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryMetadata {
    pub execution_time_ms: u64,
    pub cache_hit_rate: f64,
    pub processed_claims: usize,
    pub related_node_count: usize,
    pub conflict_count: usize,
}

/// Summary of a belief
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeliefSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_id: Option<Uuid>,
    pub content: String,
    pub confidence: f64,
    pub based_on: usize,
    pub consolidation_stage: ConsolidationStage,
    #[serde(default)]
    pub claim_ids: Vec<Uuid>,
    /// Reliability label: "high", "medium", "low", "unverified"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reliability: Option<String>,
}

/// Graph relation from activation diffusion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphRelation {
    pub content: String,
    pub relation_type: String,
    pub activation: f64,
    pub source_claim_id: Uuid,
}

/// Conflict summary for responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictSummary {
    pub id: Uuid,
    pub claim_a_summary: String,
    pub claim_b_summary: String,
    pub conflict_type: ConflictType,
    pub severity: f64,
    pub detected_at: chrono::DateTime<chrono::Utc>,
    /// LLM-based sampling analysis for deeper conflict resolution
    #[serde(default)]
    pub sampling_analysis: Option<String>,
}

impl From<&ConflictRecord> for ConflictSummary {
    fn from(conflict: &ConflictRecord) -> Self {
        Self {
            id: conflict.id,
            claim_a_summary: String::new(),
            claim_b_summary: String::new(),
            conflict_type: conflict.conflict_type,
            severity: conflict.severity,
            detected_at: conflict.detected_at,
            sampling_analysis: None,
        }
    }
}

/// Prediction result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionResult {
    pub content: String,
    pub confidence: f64,
    pub method: PredictionMethod,
    #[serde(default)]
    pub based_on_claims: Vec<Uuid>,
    /// LLM-based sampling analysis for enhanced prediction
    #[serde(default)]
    pub sampling_analysis: Option<String>,
}

/// Prediction method used
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredictionMethod {
    LlmBeliefContext,
    DedicatedModel,
    StatisticalTrend,
}

/// Freshness information
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FreshnessInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub newest_source: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oldest_source: Option<chrono::DateTime<chrono::Utc>>,
    pub staleness_warning: bool,
}

/// Cache status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheStatus {
    Hit,
    Miss,
    Stale,
}

/// Cognitive path used for query processing (S6: dual-path cognition)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CognitivePath {
    /// System 1: Fast path — high-confidence cache hit, no full reasoning
    System1,
    /// System 2: Slow path — full vector search + graph diffusion + LLM reasoning
    System2,
}

/// Thresholds for System 1/2 path decision
pub struct DualPathThresholds {
    /// Minimum cache confidence for System 1 fast path
    pub system1_confidence: f64,
    /// Minimum success rate for System 1 (only applied when hit_count >= min_hits)
    pub system1_success_rate: f64,
    /// Minimum hits before success rate is considered
    pub min_hits_for_rate: u64,
}

impl Default for DualPathThresholds {
    fn default() -> Self {
        Self {
            system1_confidence: 0.7,
            system1_success_rate: 0.5,
            min_hits_for_rate: 5,
        }
    }
}

impl DualPathThresholds {
    /// Load from environment or use defaults
    pub fn from_env() -> Self {
        Self {
            system1_confidence: std::env::var("SYSTEM1_CONFIDENCE_THRESHOLD")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.7),
            system1_success_rate: std::env::var("SYSTEM1_SUCCESS_RATE_THRESHOLD")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.5),
            min_hits_for_rate: std::env::var("SYSTEM1_MIN_HITS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5),
        }
    }
}

impl QueryCacheEntry {
    /// Determine if this cache entry qualifies for System 1 fast path
    pub fn qualifies_for_system1(&self, thresholds: &DualPathThresholds) -> bool {
        // Must have sufficient confidence
        if self.confidence < thresholds.system1_confidence {
            return false;
        }

        // If enough hits, check success rate
        if self.hit_count >= thresholds.min_hits_for_rate
            && self.success_rate() < thresholds.system1_success_rate
        {
            return false;
        }

        true
    }
}

/// Query cache entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryCacheEntry {
    pub query_hash: u64,
    pub response: McpQueryResponse,
    pub confidence: f64,
    pub hit_count: u64,
    pub success_count: u64,
    pub last_used: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalidated_by: Option<Uuid>,
}

impl QueryCacheEntry {
    /// Create a new cache entry
    pub fn new(query_hash: u64, response: McpQueryResponse) -> Self {
        let now = chrono::Utc::now();
        // Cache confidence derived from best_belief (enables System 1 on high-confidence results)
        let confidence = response
            .best_belief
            .as_ref()
            .map(|b| b.confidence)
            .unwrap_or(0.5);
        Self {
            query_hash,
            response,
            confidence,
            hit_count: 0,
            success_count: 0,
            last_used: now,
            created_at: now,
            invalidated_by: None,
        }
    }

    /// Record a hit
    pub fn record_hit(&mut self) {
        self.hit_count += 1;
        self.last_used = chrono::Utc::now();
    }

    /// Record success feedback
    pub fn record_success(&mut self) {
        self.success_count += 1;
    }

    /// Calculate success rate
    pub fn success_rate(&self) -> f64 {
        if self.hit_count == 0 {
            0.0
        } else {
            self.success_count as f64 / self.hit_count as f64
        }
    }

    /// Check if cache entry is valid (not expired)
    pub fn is_valid(&self, ttl_seconds: i64) -> bool {
        let age = chrono::Utc::now() - self.created_at;

        // 1. Check TTL
        if age.num_seconds() >= ttl_seconds {
            return false;
        }

        // 2. Check manual invalidation
        if self.invalidated_by.is_some() {
            return false;
        }

        // 3. Check performance metrics
        // If we have very few hits, we don't have enough data to judge success rate
        // So we trust the initial confidence
        if self.hit_count < 5 {
            return self.confidence >= 0.4;
        }

        // 4. Check success rate for established entries
        // If the success rate is too low, the entry is no longer considered valid
        self.success_rate() >= 0.3 && self.confidence >= 0.3
    }
}
