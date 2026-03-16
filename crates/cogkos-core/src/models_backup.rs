//! Core domain models for CogKOS

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier type
pub type Id = Uuid;

/// Tenant identifier
pub type TenantId = String;

/// Confidence score (0.0 - 1.0)
pub type Confidence = f64;

/// Node type in knowledge graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Entity,
    Relation,
    Event,
    Attribute,
    Prediction,
    Insight,
    File,
}

impl Default for NodeType {
    fn default() -> Self {
        Self::Entity
    }
}

/// Epistemic status of a claim
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpistemicStatus {
    Asserted,
    Corroborated,
    Contested,
    Retracted,
    Superseded,
}

impl Default for EpistemicStatus {
    fn default() -> Self {
        Self::Asserted
    }
}

/// Consolidation stage of a claim
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ConsolidationStage {
    /// Raw assertion, fast-track insertion
    FastTrack,
    /// Consolidated belief from multiple assertions
    Consolidated,
    /// Cross-domain insight
    Insight,
    /// Archived knowledge
    Archived,
}

impl Default for ConsolidationStage {
    fn default() -> Self {
        Self::FastTrack
    }
}

/// Source of the claim
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Claimant {
    Human {
        user_id: String,
        role: String,
    },
    Agent {
        agent_id: String,
        model: String,
    },
    System,
    ExternalPublic {
        source_name: String,
    },
}

impl Claimant {
    /// Get base trust level for this claimant type
    pub fn base_trust(&self) -> Confidence {
        match self {
            Claimant::Human { role, .. } => match role.as_str() {
                "expert" => 0.85,
                "admin" => 0.80,
                _ => 0.70,
            },
            Claimant::Agent { .. } => 0.75,
            Claimant::System => 0.90,
            Claimant::ExternalPublic { .. } => 0.60,
        }
    }

    /// Get source ID string
    pub fn source_id(&self) -> String {
        match self {
            Claimant::Human { user_id, .. } => user_id.clone(),
            Claimant::Agent { agent_id, .. } => agent_id.clone(),
            Claimant::System => "system".to_string(),
            Claimant::ExternalPublic { source_name } => source_name.clone(),
        }
    }
}

/// Type of knowledge source (legacy, use Claimant instead)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    Human,
    Agent,
    Document,
    ExternalApi,
    Prediction,
    System,
}

/// Provenance information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    pub origin_url: Option<String>,
    pub document_id: Option<Id>,
    pub extraction_method: String,
    pub extracted_at: DateTime<Utc>,
    pub raw_chunk: Option<String>,
}

impl Default for Provenance {
    fn default() -> Self {
        Self {
            origin_url: None,
            document_id: None,
            extraction_method: "manual".to_string(),
            extracted_at: Utc::now(),
            raw_chunk: None,
        }
    }
}

/// Visibility level for access control
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Private,
    Team,
    Tenant,
    CrossTenant,
    Public,
}

impl Default for Visibility {
    fn default() -> Self {
        Self::Tenant
    }
}

/// Access control envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessEnvelope {
    pub visibility: Visibility,
    pub tenant_id: String,
    pub allowed_roles: Vec<String>,
    pub gdpr_applicable: bool,
}

impl AccessEnvelope {
    pub fn new(tenant_id: impl Into<String>) -> Self {
        Self {
            visibility: Visibility::Tenant,
            tenant_id: tenant_id.into(),
            allowed_roles: vec![],
            gdpr_applicable: false,
        }
    }

    pub fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }

    pub fn with_roles(mut self, roles: Vec<String>) -> Self {
        self.allowed_roles = roles;
        self
    }

    pub fn with_gdpr(mut self, applicable: bool) -> Self {
        self.gdpr_applicable = applicable;
        self
    }
}

impl Default for AccessEnvelope {
    fn default() -> Self {
        Self::new("default")
    }
}

/// GDPR data categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GdprCategory {
    Personal,
    Sensitive,
    Public,
    Anonymized,
}

/// Provenance record (new version)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceRecord {
    pub source_id: String,
    pub source_type: String,
    pub ingestion_method: String,
    pub original_url: Option<String>,
    pub audit_hash: String,
}

impl ProvenanceRecord {
    pub fn new(source_id: impl Into<String>, source_type: impl Into<String>) -> Self {
        Self {
            source_id: source_id.into(),
            source_type: source_type.into(),
            ingestion_method: "manual".to_string(),
            original_url: None,
            audit_hash: String::new(),
        }
    }

    pub fn with_method(mut self, method: impl Into<String>) -> Self {
        self.ingestion_method = method.into();
        self
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.original_url = Some(url.into());
        self
    }

    pub fn with_hash(mut self, hash: impl Into<String>) -> Self {
        self.audit_hash = hash.into();
        self
    }
}

/// Epistemic claim - the fundamental knowledge unit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpistemicClaim {
    pub id: Id,
    pub tenant_id: TenantId,
    pub content: String,
    pub node_type: NodeType,
    pub epistemic_status: EpistemicStatus,
    pub confidence: Confidence,
    pub consolidation_stage: ConsolidationStage,
    pub claimant: Claimant,
    pub provenance: ProvenanceRecord,
    pub access_envelope: AccessEnvelope,
    pub activation_weight: f64,
    pub access_count: u64,
    pub last_accessed: Option<DateTime<Utc>>,
    pub t_valid_start: DateTime<Utc>,
    pub t_valid_end: Option<DateTime<Utc>>,
    pub t_known: DateTime<Utc>,
    pub vector_id: Option<Uuid>,
    pub last_prediction_error: Option<f64>,
    pub derived_from: Vec<Uuid>,
    pub needs_revalidation: bool,
    pub durability: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: ClaimMetadata,
}

impl EpistemicClaim {
    pub fn new(
        tenant_id: TenantId,
        content: String,
        node_type: NodeType,
        claimant: Claimant,
        access_envelope: AccessEnvelope,
        provenance: ProvenanceRecord,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            tenant_id,
            content,
            node_type,
            epistemic_status: EpistemicStatus::Asserted,
            confidence: claimant.base_trust(),
            consolidation_stage: ConsolidationStage::FastTrack,
            claimant,
            provenance,
            access_envelope,
            activation_weight: 0.5,
            access_count: 0,
            last_accessed: None,
            t_valid_start: now,
            t_valid_end: None,
            t_known: now,
            vector_id: None,
            last_prediction_error: None,
            derived_from: vec![],
            needs_revalidation: false,
            durability: 1.0,
            created_at: now,
            updated_at: now,
            metadata: ClaimMetadata::default(),
        }
    }

    /// Check if the claim has expired
    pub fn is_expired(&self) -> bool {
        self.t_valid_end.map(|exp| exp < Utc::now()).unwrap_or(false)
    }

    /// Update confidence with new evidence (Bayesian update)
    pub fn update_confidence(&mut self, new_evidence_confidence: f64, source_trust: f64) {
        // Simple Bayesian update approximation
        let prior = self.confidence;
        let likelihood = new_evidence_confidence * source_trust;
        let posterior = (prior * likelihood) / (prior * likelihood + (1.0 - prior) * (1.0 - likelihood));
        self.confidence = posterior.clamp(0.0, 1.0);
        self.updated_at = Utc::now();
    }

    /// Record access (S3: read equals write)
    pub fn record_access(&mut self) {
        self.access_count += 1;
        self.last_accessed = Some(Utc::now());
        // Increment activation weight with ceiling
        self.activation_weight = (self.activation_weight + 0.1).min(1.0);
        self.updated_at = Utc::now();
    }

    /// Apply knowledge decay
    pub fn apply_decay(&mut self, decay_factor: f64) {
        self.activation_weight *= decay_factor;
        self.durability *= decay_factor;
        // Confidence also decays slowly
        self.confidence = (self.confidence * decay_factor).max(0.1);
        self.updated_at = Utc::now();
    }
}

/// Additional claim metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClaimMetadata {
    pub tags: Vec<String>,
    pub domain: Option<String>,
    pub entity_refs: Vec<String>,
    pub prediction: Option<Prediction>,
}

/// Prediction metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    pub predicted_outcome: String,
    pub predicted_at: DateTime<Utc>,
    pub validation_deadline: DateTime<Utc>,
    pub validated: Option<bool>,
    pub actual_outcome: Option<String>,
    pub error_score: Option<f64>,
}

/// Conflict type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictType {
    DirectContradiction,
    TemporalInconsistency,
    ConfidenceMismatch,
    ContextualDifference,
    SourceDisagreement,
    TemporalShift,
}

/// Conflict resolution method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionMethod {
    ManualReview,
    AutomatedWeighted,
    TemporalPriority,
    SourceAuthority,
}

/// Conflict resolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resolution {
    pub method: ResolutionMethod,
    pub winning_claim_id: Id,
    pub explanation: String,
    pub resolved_by: String,
}

/// Resolution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionStatus {
    Open,
    Elevated,
    Dismissed,
    Accepted,
}

/// Conflict record between claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictRecord {
    pub id: Id,
    pub tenant_id: TenantId,
    pub claim_a_id: Id,
    pub claim_b_id: Id,
    pub conflict_type: ConflictType,
    pub severity: f64,
    pub description: String,
    pub detected_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolution: Option<Resolution>,
    pub resolution_status: ResolutionStatus,
    pub elevated_insight_id: Option<Id>,
}

impl ConflictRecord {
    pub fn new(
        tenant_id: TenantId,
        claim_a_id: Id,
        claim_b_id: Id,
        conflict_type: ConflictType,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            tenant_id,
            claim_a_id,
            claim_b_id,
            conflict_type,
            severity: 0.5,
            description: description.into(),
            detected_at: Utc::now(),
            resolved_at: None,
            resolution: None,
            resolution_status: ResolutionStatus::Open,
            elevated_insight_id: None,
        }
    }

    pub fn with_severity(mut self, severity: f64) -> Self {
        self.severity = severity.clamp(0.0, 1.0);
        self
    }
}

/// Query request from MCP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    pub tenant_id: TenantId,
    pub query: String,
    pub context: QueryContext,
    pub include_predictions: bool,
    pub include_conflicts: bool,
    pub include_gaps: bool,
}

impl Default for QueryRequest {
    fn default() -> Self {
        Self {
            tenant_id: "default".to_string(),
            query: String::new(),
            context: QueryContext::default(),
            include_predictions: true,
            include_conflicts: true,
            include_gaps: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryContext {
    pub domain: Option<String>,
    pub urgency: Urgency,
    pub max_results: usize,
    pub filters: Vec<QueryFilter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Urgency {
    #[default]
    Normal,
    Low,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QueryFilter {
    Stage { stage: ConsolidationStage },
    Confidence { min: f64, max: f64 },
    DateRange { from: DateTime<Utc>, to: DateTime<Utc> },
    Source { source_type: SourceType },
    Tag { tag: String },
}

/// Cache status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheStatus {
    Hit,
    Miss,
    Stale,
}

/// Belief summary for query response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeliefSummary {
    pub claim_id: Option<Id>,
    pub content: String,
    pub confidence: Confidence,
    pub based_on: usize,
    pub consolidation_stage: ConsolidationStage,
    pub claim_ids: Vec<Id>,
}

/// Graph relation for query response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphRelation {
    pub claim_id: Id,
    pub content: String,
    pub relation_type: String,
    pub activation: f64,
}

/// Conflict summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictSummary {
    pub claim_a: String,
    pub claim_b: String,
    pub conflict_type: ConflictType,
    pub severity: f64,
}

impl From<&ConflictRecord> for ConflictSummary {
    fn from(record: &ConflictRecord) -> Self {
        Self {
            claim_a: record.claim_a_id.to_string(),
            claim_b: record.claim_b_id.to_string(),
            conflict_type: record.conflict_type,
            severity: record.severity,
        }
    }
}

/// Prediction method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredictionMethod {
    LlmBeliefContext,
    DedicatedModel,
    StatisticalTrend,
}

/// Prediction result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionResult {
    pub content: String,
    pub confidence: Confidence,
    pub method: PredictionMethod,
    pub based_on_claims: Vec<Id>,
}

/// Freshness info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreshnessInfo {
    pub newest_source: Option<DateTime<Utc>>,
    pub oldest_source: Option<DateTime<Utc>>,
    pub staleness_warning: bool,
}

/// MCP Query Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpQueryResponse {
    pub query_context: String,
    pub best_belief: Option<BeliefSummary>,
    pub related_by_graph: Vec<GraphRelation>,
    pub conflicts: Vec<ConflictSummary>,
    pub prediction: Option<PredictionResult>,
    pub knowledge_gaps: Vec<String>,
    pub freshness: FreshnessInfo,
    pub cache_status: CacheStatus,
}

/// Query cache entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryCacheEntry {
    pub query_hash: u64,
    pub response: McpQueryResponse,
    pub confidence: f64,
    pub hit_count: u64,
    pub success_count: u64,
    pub last_used: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub invalidated_by: Option<Uuid>,
}

impl QueryCacheEntry {
    pub fn new(query_hash: u64, response: McpQueryResponse) -> Self {
        let now = Utc::now();
        Self {
            query_hash,
            response,
            confidence: 0.6,
            hit_count: 0,
            success_count: 0,
            last_used: now,
            created_at: now,
            invalidated_by: None,
        }
    }

    pub fn record_hit(&mut self) {
        self.hit_count += 1;
        self.last_used = Utc::now();
    }

    pub fn record_success(&mut self) {
        self.success_count += 1;
    }

    pub fn success_rate(&self) -> f64 {
        if self.hit_count == 0 {
            0.0
        } else {
            self.success_count as f64 / self.hit_count as f64
        }
    }

    pub fn is_stale(&self, ttl_seconds: i64) -> bool {
        let age = Utc::now() - self.created_at;
        age.num_seconds() > ttl_seconds
    }
}

/// Agent feedback
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentFeedback {
    pub query_hash: u64,
    pub agent_id: String,
    pub success: bool,
    pub feedback_note: String,
    pub timestamp: DateTime<Utc>,
}

impl AgentFeedback {
    pub fn new(query_hash: u64, agent_id: impl Into<String>, success: bool) -> Self {
        Self {
            query_hash,
            agent_id: agent_id.into(),
            success,
            feedback_note: String::new(),
            timestamp: Utc::now(),
        }
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.feedback_note = note.into();
        self
    }
}

/// API Key for authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: Id,
    pub key_hash: String,
    pub tenant_id: TenantId,
    pub name: String,
    pub permissions: Vec<Permission>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub enabled: bool,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Permission types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Read,
    Write,
    Admin,
    Delete,
}

/// Subscription source type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionType {
    Rss,
    ApiPoll,
    WebScraping,
    SearchAlert,
}

/// Subscription source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionSource {
    pub id: Id,
    pub name: String,
    pub source_type: SubscriptionType,
    pub config: serde_json::Value,
    pub poll_interval_secs: u64,
    pub claimant_template: Claimant,
    pub base_confidence: f64,
    pub enabled: bool,
    pub last_polled: Option<DateTime<Utc>>,
    pub error_count: u32,
    pub tenant_id: String,
}

/// Evolution mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionMode {
    Incremental,
    ParadigmShift,
}

/// Paradigm shift result
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShiftResult {
    Success,
    Rollback,
}

/// Shift record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftRecord {
    pub timestamp: DateTime<Utc>,
    pub result: ShiftResult,
    pub old_framework_hash: String,
    pub new_framework_hash: Option<String>,
    pub improvement_pct: Option<f64>,
}

/// Anomaly signals for evolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalySignals {
    pub prediction_error_streak: u32,
    pub conflict_density_pct: f64,
    pub cache_hit_rate_trend: f64,
}

/// Evolution engine state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionEngineState {
    pub mode: EvolutionMode,
    pub anomaly_counter: u32,
    pub paradigm_shift_threshold: u32,
    pub ticks_since_last_shift: u32,
    pub shift_history: Vec<ShiftRecord>,
}

impl Default for EvolutionEngineState {
    fn default() -> Self {
        Self {
            mode: EvolutionMode::Incremental,
            anomaly_counter: 0,
            paradigm_shift_threshold: 100,
            ticks_since_last_shift: 0,
            shift_history: vec![],
        }
    }
}

/// Meta knowledge entry for federation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaKnowledgeEntry {
    pub instance_id: String,
    pub domain_tags: Vec<String>,
    pub expertise_score: f64,
    pub last_updated: DateTime<Utc>,
}

/// Federation health check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationHealthCheck {
    pub diversity_entropy: f64,
    pub independence_score: f64,
    pub centralization_gini: f64,
    pub aggregation_vs_best: f64,
}

/// Query response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    pub claims: Vec<EpistemicClaim>,
    pub total: usize,
    pub page: usize,
    pub per_page: usize,
}

/// Vector match result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorMatch {
    pub id: Id,
    pub score: f32,
}

/// Graph node with activation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: Id,
    pub content: String,
    pub activation: f64,
}
