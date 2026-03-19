//! CogKOS Store - Storage abstraction layer
//!
//! Provides unified access to PostgreSQL, FalkorDB, Qdrant, and S3.

pub mod graph;
pub mod postgres;
pub mod prediction_history;
pub mod s3;
pub mod vector;

// Re-export audit types from cogkos-core
pub use cogkos_core::audit::{
    AuditActor, AuditCategory, AuditEntry, AuditFilter, AuditOutcome, AuditSeverity, AuditStore,
    AuditTarget,
};

// Re-export PostgreSQL audit store
pub mod postgres_audit;
pub use postgres_audit::PostgresAuditStore;

pub use async_trait::async_trait;
use cogkos_core::Result;
use cogkos_core::models::*;

/// Unified store trait
#[async_trait]
pub trait Store: Send + Sync {
    /// Create a new claim
    async fn create_claim(&self, claim: &EpistemicClaim) -> Result<Id>;

    /// Get claim by ID
    async fn get_claim(&self, tenant_id: &TenantId, id: Id) -> Result<EpistemicClaim>;

    /// Update claim
    async fn update_claim(&self, claim: &EpistemicClaim) -> Result<()>;

    /// Delete claim
    async fn delete_claim(&self, tenant_id: &TenantId, id: Id) -> Result<()>;

    /// Query claims
    async fn query_claims(&self, req: &QueryRequest) -> Result<QueryResponse>;

    /// Create conflict record
    async fn create_conflict(&self, conflict: &ConflictRecord) -> Result<Id>;

    /// Get conflicts for a claim
    async fn get_conflicts(
        &self,
        tenant_id: &TenantId,
        claim_id: Id,
    ) -> Result<Vec<ConflictRecord>>;

    /// Validate API key
    async fn validate_api_key(&self, key_hash: &str) -> Result<ApiKey>;

    /// Update API key last used
    async fn update_api_key_usage(&self, key_id: Id) -> Result<()>;
}

/// Claim store trait
#[async_trait]
pub trait ClaimStore: Send + Sync {
    async fn insert_claim(&self, claim: &EpistemicClaim) -> Result<Id>;
    async fn get_claim(&self, id: Id, tenant_id: &str) -> Result<EpistemicClaim>;
    async fn update_claim(&self, claim: &EpistemicClaim) -> Result<()>;
    async fn delete_claim(&self, id: Id, tenant_id: &str) -> Result<()>;
    async fn query_claims(
        &self,
        tenant_id: &str,
        filters: &[QueryFilter],
    ) -> Result<Vec<EpistemicClaim>>;
    async fn update_activation(&self, id: Id, tenant_id: &str, delta: f64) -> Result<()>;
    async fn get_conflicts_for_claim(
        &self,
        claim_id: Id,
        tenant_id: &str,
    ) -> Result<Vec<ConflictRecord>>;

    async fn list_claims_needing_confidence_boost(
        &self,
        tenant_id: &str,
        limit: usize,
    ) -> Result<Vec<EpistemicClaim>>;

    // Additional methods needed by other crates
    async fn list_claims_by_stage(
        &self,
        tenant_id: &str,
        stage: cogkos_core::ConsolidationStage,
        limit: usize,
    ) -> Result<Vec<EpistemicClaim>>;
    async fn search_claims(
        &self,
        tenant_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<EpistemicClaim>>;
    async fn update_confidence(&self, id: Id, tenant_id: &str, confidence: f64) -> Result<()>;
    async fn list_claims_needing_revalidation(
        &self,
        tenant_id: &str,
        threshold: f64,
        limit: usize,
    ) -> Result<Vec<EpistemicClaim>>;
    async fn list_tenants(&self) -> Result<Vec<String>>;
    async fn insert_conflict(&self, conflict: &ConflictRecord) -> Result<()>;
    async fn resolve_conflict(
        &self,
        conflict_id: uuid::Uuid,
        tenant_id: &str,
        status: ResolutionStatus,
        note: Option<String>,
    ) -> Result<()>;
}

/// Memory layer store trait — separated from ClaimStore for explicit opt-in
#[async_trait]
pub trait MemoryLayerStore: Send + Sync {
    async fn list_claims_by_memory_layer(
        &self,
        tenant_id: &str,
        memory_layer: &str,
        session_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EpistemicClaim>>;

    async fn count_claims_by_memory_layer(
        &self,
        tenant_id: &str,
        memory_layer: &str,
        session_id: Option<&str>,
    ) -> Result<usize>;

    /// Delete expired claims by memory layer (based on max_age_hours).
    async fn gc_expired_memory_layer(
        &self,
        tenant_id: &str,
        memory_layer: &str,
        max_age_hours: f64,
    ) -> Result<usize>;

    /// Promote claims: set metadata memory_layer from `from_layer` to `to_layer`
    /// where rehearsal_count >= threshold. Returns count of promoted claims.
    async fn promote_memory_layer(
        &self,
        tenant_id: &str,
        from_layer: &str,
        to_layer: &str,
        min_rehearsal_count: u64,
    ) -> Result<usize>;
}

/// Vector store trait
#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn upsert(&self, id: Id, vector: Vec<f32>, metadata: serde_json::Value) -> Result<()>;
    async fn search(
        &self,
        vector: Vec<f32>,
        tenant_id: &str,
        limit: u32,
    ) -> Result<Vec<VectorMatch>>;
    async fn delete(&self, id: Id) -> Result<()>;

    // Additional methods needed by other crates
    async fn calculate_novelty(&self, vector: Vec<f32>, tenant_id: &str) -> Result<f64>;
}

/// Graph store trait
#[async_trait]
pub trait GraphStore: Send + Sync {
    async fn add_node(&self, claim: &EpistemicClaim) -> Result<()>;
    async fn add_edge(&self, from: Id, to: Id, relation: &str, weight: f64) -> Result<()>;
    async fn find_related(&self, id: Id, depth: u32, min_activation: f64)
    -> Result<Vec<GraphNode>>;
    async fn find_path(&self, from: Id, to: Id) -> Result<Vec<GraphNode>>;

    // Additional methods needed by other crates
    async fn upsert_node(&self, claim: &EpistemicClaim) -> Result<()>;
    async fn create_edge(&self, from: Id, to: Id, relation: &str, weight: f64) -> Result<()>;

    /// Graph activation diffusion - spread activation along graph edges
    ///
    /// Implements the algorithm from ARCHITECTURE.md:
    /// - Vector-matched node A (activation=1.0)
    ///   -> A --CAUSES--> B (weight=0.8) => B.activation += 1.0 * 0.8 * decay
    ///   -> B --SIMILAR_TO--> C (weight=0.6) => C.activation += ...
    ///   -> Collect all nodes with activation > threshold
    async fn activation_diffusion(
        &self,
        start_id: Id,
        initial_activation: f64,
        depth: u32,
        decay_factor: f64,
        min_threshold: f64,
    ) -> Result<Vec<GraphNode>>;
}

/// Cache store trait
#[async_trait]
pub trait CacheStore: Send + Sync {
    async fn get_cached(&self, tenant_id: &str, query_hash: u64)
    -> Result<Option<QueryCacheEntry>>;
    async fn set_cached(&self, tenant_id: &str, entry: &QueryCacheEntry) -> Result<()>;
    async fn record_hit(&self, tenant_id: &str, query_hash: u64) -> Result<()>;
    async fn record_success(&self, tenant_id: &str, query_hash: u64) -> Result<()>;
    async fn invalidate(&self, tenant_id: &str, query_hash: u64) -> Result<()>;
    /// Update the TTL (time-to-live) of a cache entry, effectively refreshing its last access time
    async fn refresh_ttl(&self, tenant_id: &str, query_hash: u64) -> Result<()>;
}

/// Feedback store trait
#[async_trait]
pub trait FeedbackStore: Send + Sync {
    async fn insert_feedback(&self, feedback: &AgentFeedback) -> Result<()>;
    async fn get_feedback_for_query(&self, query_hash: u64) -> Result<Vec<AgentFeedback>>;
}

/// Object store trait
#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn upload(&self, key: &str, data: &[u8], content_type: &str) -> Result<String>;
    async fn download(&self, key: &str) -> Result<Vec<u8>>;
    async fn delete(&self, key: &str) -> Result<()>;
    async fn presigned_url(&self, key: &str, expiry_secs: u64) -> Result<String>;
}

/// Auth store trait
#[async_trait]
pub trait AuthStore: Send + Sync {
    async fn validate_api_key(&self, api_key: &str) -> Result<(String, Vec<String>)>;
    async fn create_api_key(&self, tenant_id: &str, permissions: Vec<String>) -> Result<String>;
    async fn revoke_api_key(&self, key_hash: &str) -> Result<()>;
}

/// Combined stores container
#[derive(Clone)]
pub struct Stores {
    pub claims: std::sync::Arc<dyn ClaimStore>,
    pub vectors: std::sync::Arc<dyn VectorStore>,
    pub graph: std::sync::Arc<dyn GraphStore>,
    pub cache: std::sync::Arc<dyn CacheStore>,
    pub feedback: std::sync::Arc<dyn FeedbackStore>,
    pub objects: std::sync::Arc<dyn ObjectStore>,
    pub auth: std::sync::Arc<dyn AuthStore>,
    pub gaps: std::sync::Arc<dyn GapStore>,
    pub audit: std::sync::Arc<dyn AuditStore>,
    pub subscription: std::sync::Arc<dyn SubscriptionStore>,
    pub memory_layers: std::sync::Arc<dyn MemoryLayerStore>,
}

impl Stores {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        claims: std::sync::Arc<dyn ClaimStore>,
        vectors: std::sync::Arc<dyn VectorStore>,
        graph: std::sync::Arc<dyn GraphStore>,
        cache: std::sync::Arc<dyn CacheStore>,
        feedback: std::sync::Arc<dyn FeedbackStore>,
        objects: std::sync::Arc<dyn ObjectStore>,
        auth: std::sync::Arc<dyn AuthStore>,
        gaps: std::sync::Arc<dyn GapStore>,
        audit: std::sync::Arc<dyn AuditStore>,
        subscription: std::sync::Arc<dyn SubscriptionStore>,
        memory_layers: std::sync::Arc<dyn MemoryLayerStore>,
    ) -> Self {
        Self {
            claims,
            vectors,
            graph,
            cache,
            feedback,
            objects,
            auth,
            gaps,
            audit,
            subscription,
            memory_layers,
        }
    }
}

// In-memory feedback store for testing/development
use cogkos_core::models::AgentFeedback;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// In-memory feedback store
pub struct InMemoryFeedbackStore {
    feedback: Arc<RwLock<HashMap<u64, Vec<AgentFeedback>>>>,
}

impl InMemoryFeedbackStore {
    pub fn new() -> Self {
        Self {
            feedback: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryFeedbackStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FeedbackStore for InMemoryFeedbackStore {
    async fn insert_feedback(&self, feedback: &AgentFeedback) -> Result<()> {
        let mut store = self.feedback.write().await;
        let entry = store.entry(feedback.query_hash).or_insert_with(Vec::new);
        entry.push(feedback.clone());
        Ok(())
    }

    async fn get_feedback_for_query(&self, query_hash: u64) -> Result<Vec<AgentFeedback>> {
        let store = self.feedback.read().await;
        Ok(store.get(&query_hash).cloned().unwrap_or_default())
    }
}

/// In-memory claim store
pub struct InMemoryClaimStore {
    claims: Arc<RwLock<HashMap<Id, EpistemicClaim>>>,
    conflicts: Arc<RwLock<HashMap<Id, Vec<ConflictRecord>>>>,
}

impl InMemoryClaimStore {
    pub fn new() -> Self {
        Self {
            claims: Arc::new(RwLock::new(HashMap::new())),
            conflicts: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryClaimStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ClaimStore for InMemoryClaimStore {
    async fn insert_claim(&self, claim: &EpistemicClaim) -> Result<Id> {
        let mut claims = self.claims.write().await;
        claims.insert(claim.id, claim.clone());
        Ok(claim.id)
    }

    async fn get_claim(&self, id: Id, tenant_id: &str) -> Result<EpistemicClaim> {
        let claims = self.claims.read().await;
        let claim = claims
            .get(&id)
            .ok_or_else(|| cogkos_core::CogKosError::NotFound(format!("Claim {} not found", id)))?;
        if claim.tenant_id != tenant_id {
            return Err(cogkos_core::CogKosError::AccessDenied(
                "Tenant mismatch".to_string(),
            ));
        }
        Ok(claim.clone())
    }

    async fn update_claim(&self, claim: &EpistemicClaim) -> Result<()> {
        let mut claims = self.claims.write().await;
        claims.insert(claim.id, claim.clone());
        Ok(())
    }

    async fn delete_claim(&self, id: Id, _tenant_id: &str) -> Result<()> {
        let mut claims = self.claims.write().await;
        claims.remove(&id);
        Ok(())
    }

    async fn query_claims(
        &self,
        tenant_id: &str,
        _filters: &[QueryFilter],
    ) -> Result<Vec<EpistemicClaim>> {
        let claims = self.claims.read().await;
        Ok(claims
            .values()
            .filter(|c| c.tenant_id == tenant_id)
            .cloned()
            .collect())
    }

    async fn update_activation(&self, id: Id, _tenant_id: &str, delta: f64) -> Result<()> {
        let mut claims = self.claims.write().await;
        if let Some(claim) = claims.get_mut(&id) {
            claim.record_access(delta);
        }
        Ok(())
    }

    async fn get_conflicts_for_claim(
        &self,
        claim_id: Id,
        _tenant_id: &str,
    ) -> Result<Vec<ConflictRecord>> {
        let conflicts = self.conflicts.read().await;
        Ok(conflicts.get(&claim_id).cloned().unwrap_or_default())
    }

    async fn list_claims_by_stage(
        &self,
        tenant_id: &str,
        stage: ConsolidationStage,
        limit: usize,
    ) -> Result<Vec<EpistemicClaim>> {
        let claims = self.claims.read().await;
        let mut result: Vec<_> = claims
            .values()
            .filter(|c| c.tenant_id == tenant_id && c.consolidation_stage == stage)
            .cloned()
            .collect();
        result.truncate(limit);
        Ok(result)
    }

    async fn search_claims(
        &self,
        tenant_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<EpistemicClaim>> {
        let claims = self.claims.read().await;
        let mut result: Vec<_> = claims
            .values()
            .filter(|c| c.tenant_id == tenant_id && c.content.contains(query))
            .cloned()
            .collect();
        result.truncate(limit);
        Ok(result)
    }

    async fn update_confidence(&self, id: Id, _tenant_id: &str, confidence: f64) -> Result<()> {
        let mut claims = self.claims.write().await;
        if let Some(claim) = claims.get_mut(&id) {
            claim.confidence = confidence;
        }
        Ok(())
    }

    async fn list_claims_needing_revalidation(
        &self,
        tenant_id: &str,
        _threshold: f64,
        limit: usize,
    ) -> Result<Vec<EpistemicClaim>> {
        let claims = self.claims.read().await;
        let mut result: Vec<_> = claims
            .values()
            .filter(|c| c.tenant_id == tenant_id && c.needs_revalidation)
            .cloned()
            .collect();
        result.truncate(limit);
        Ok(result)
    }

    async fn list_tenants(&self) -> Result<Vec<String>> {
        let claims = self.claims.read().await;
        let tenants: std::collections::HashSet<_> =
            claims.values().map(|c| c.tenant_id.clone()).collect();
        Ok(tenants.into_iter().collect())
    }

    async fn insert_conflict(&self, conflict: &ConflictRecord) -> Result<()> {
        let mut conflicts = self.conflicts.write().await;
        conflicts
            .entry(conflict.claim_a_id)
            .or_default()
            .push(conflict.clone());
        conflicts
            .entry(conflict.claim_b_id)
            .or_default()
            .push(conflict.clone());
        Ok(())
    }

    async fn resolve_conflict(
        &self,
        conflict_id: uuid::Uuid,
        _tenant_id: &str,
        status: ResolutionStatus,
        note: Option<String>,
    ) -> Result<()> {
        let mut conflicts = self.conflicts.write().await;
        for records in conflicts.values_mut() {
            for record in records.iter_mut() {
                if record.id == conflict_id {
                    record.resolution_status = status;
                    record.resolved_at = Some(chrono::Utc::now());
                    record.resolution_note = note.clone();
                }
            }
        }
        Ok(())
    }

    async fn list_claims_needing_confidence_boost(
        &self,
        tenant_id: &str,
        limit: usize,
    ) -> Result<Vec<EpistemicClaim>> {
        let claims = self.claims.read().await;
        let mut result: Vec<_> = claims
            .values()
            .filter(|c| {
                c.tenant_id == tenant_id
                    && c.metadata
                        .get("needs_confidence_boost")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
            })
            .cloned()
            .collect();
        result.truncate(limit);
        Ok(result)
    }
}

/// In-memory cache store
pub struct InMemoryCacheStore {
    cache: Arc<RwLock<HashMap<(String, u64), QueryCacheEntry>>>,
}

impl InMemoryCacheStore {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryCacheStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CacheStore for InMemoryCacheStore {
    async fn get_cached(
        &self,
        tenant_id: &str,
        query_hash: u64,
    ) -> Result<Option<QueryCacheEntry>> {
        let cache = self.cache.read().await;
        Ok(cache.get(&(tenant_id.to_string(), query_hash)).cloned())
    }

    async fn set_cached(&self, tenant_id: &str, entry: &QueryCacheEntry) -> Result<()> {
        let mut cache = self.cache.write().await;
        cache.insert((tenant_id.to_string(), entry.query_hash), entry.clone());
        Ok(())
    }

    async fn record_hit(&self, tenant_id: &str, query_hash: u64) -> Result<()> {
        let mut cache = self.cache.write().await;
        if let Some(entry) = cache.get_mut(&(tenant_id.to_string(), query_hash)) {
            entry.record_hit();
        }
        Ok(())
    }

    async fn record_success(&self, tenant_id: &str, query_hash: u64) -> Result<()> {
        let mut cache = self.cache.write().await;
        if let Some(entry) = cache.get_mut(&(tenant_id.to_string(), query_hash)) {
            entry.record_success();
        }
        Ok(())
    }

    async fn invalidate(&self, tenant_id: &str, query_hash: u64) -> Result<()> {
        let mut cache = self.cache.write().await;
        cache.remove(&(tenant_id.to_string(), query_hash));
        Ok(())
    }

    async fn refresh_ttl(&self, tenant_id: &str, query_hash: u64) -> Result<()> {
        let mut cache = self.cache.write().await;
        if let Some(entry) = cache.get_mut(&(tenant_id.to_string(), query_hash)) {
            entry.last_used = chrono::Utc::now();
        }
        Ok(())
    }
}

/// In-memory auth store
pub struct InMemoryAuthStore {
    #[allow(clippy::type_complexity)]
    keys: Arc<RwLock<HashMap<String, (String, Vec<String>)>>>, // api_key -> (tenant_id, permissions)
}

impl InMemoryAuthStore {
    pub fn new() -> Self {
        Self {
            keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryAuthStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AuthStore for InMemoryAuthStore {
    async fn validate_api_key(&self, api_key: &str) -> Result<(String, Vec<String>)> {
        let keys = self.keys.read().await;
        keys.get(api_key)
            .cloned()
            .ok_or_else(|| cogkos_core::CogKosError::AccessDenied("Invalid API key".to_string()))
    }

    async fn create_api_key(&self, tenant_id: &str, permissions: Vec<String>) -> Result<String> {
        let mut keys = self.keys.write().await;
        let api_key = uuid::Uuid::new_v4().to_string();
        keys.insert(api_key.clone(), (tenant_id.to_string(), permissions));
        Ok(api_key)
    }

    async fn revoke_api_key(&self, _key_hash: &str) -> Result<()> {
        // Simple implementation: we don't store hash here in memory version for simplicity
        Ok(())
    }
}

/// Knowledge gap record for tracking knowledge gaps
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KnowledgeGapRecord {
    pub gap_id: uuid::Uuid,
    pub tenant_id: String,
    pub domain: String,
    pub description: String,
    pub priority: String,
    pub status: String,
    pub reported_at: chrono::DateTime<chrono::Utc>,
    pub filled_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Gap store trait for knowledge gap persistence
#[async_trait]
pub trait GapStore: Send + Sync {
    /// Record a new knowledge gap
    async fn record_gap(&self, gap: &KnowledgeGapRecord) -> Result<uuid::Uuid>;

    /// Check if a similar gap already exists (for deduplication)
    async fn find_similar_gap(
        &self,
        tenant_id: &str,
        domain: &str,
        description: &str,
    ) -> Result<Option<KnowledgeGapRecord>>;

    /// Get all gaps for a tenant
    async fn get_gaps(&self, tenant_id: &str) -> Result<Vec<KnowledgeGapRecord>>;

    /// Get gaps by domain
    async fn get_gaps_by_domain(
        &self,
        tenant_id: &str,
        domain: &str,
    ) -> Result<Vec<KnowledgeGapRecord>>;

    /// Mark gap as filled
    async fn mark_gap_filled(&self, gap_id: uuid::Uuid) -> Result<()>;
}

/// In-memory gap store for testing/development
pub struct InMemoryGapStore {
    gaps: Arc<RwLock<HashMap<uuid::Uuid, KnowledgeGapRecord>>>,
}

impl InMemoryGapStore {
    pub fn new() -> Self {
        Self {
            gaps: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryGapStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GapStore for InMemoryGapStore {
    async fn record_gap(&self, gap: &KnowledgeGapRecord) -> Result<uuid::Uuid> {
        let mut store = self.gaps.write().await;
        store.insert(gap.gap_id, gap.clone());
        Ok(gap.gap_id)
    }

    async fn find_similar_gap(
        &self,
        tenant_id: &str,
        domain: &str,
        description: &str,
    ) -> Result<Option<KnowledgeGapRecord>> {
        let store = self.gaps.read().await;
        let found = store.values().find(|g| {
            g.tenant_id == tenant_id
                && g.domain == domain
                && g.description == description
                && g.status == "open"
        });
        Ok(found.cloned())
    }

    async fn get_gaps(&self, tenant_id: &str) -> Result<Vec<KnowledgeGapRecord>> {
        let store = self.gaps.read().await;
        let gaps: Vec<KnowledgeGapRecord> = store
            .values()
            .filter(|g| g.tenant_id == tenant_id)
            .cloned()
            .collect();
        Ok(gaps)
    }

    async fn get_gaps_by_domain(
        &self,
        tenant_id: &str,
        domain: &str,
    ) -> Result<Vec<KnowledgeGapRecord>> {
        let store = self.gaps.read().await;
        let gaps: Vec<KnowledgeGapRecord> = store
            .values()
            .filter(|g| g.tenant_id == tenant_id && g.domain == domain)
            .cloned()
            .collect();
        Ok(gaps)
    }

    async fn mark_gap_filled(&self, gap_id: uuid::Uuid) -> Result<()> {
        let mut store = self.gaps.write().await;
        if let Some(gap) = store.get_mut(&gap_id) {
            gap.status = "filled".to_string();
            gap.filled_at = Some(chrono::Utc::now());
        }
        Ok(())
    }
}

/// Subscription store trait for external knowledge source persistence
#[async_trait]
pub trait SubscriptionStore: Send + Sync {
    /// Create a new subscription
    async fn create_subscription(&self, subscription: &SubscriptionSource) -> Result<uuid::Uuid>;

    /// Get subscription by ID
    async fn get_subscription(&self, tenant_id: &str, id: uuid::Uuid)
    -> Result<SubscriptionSource>;

    /// Update subscription
    async fn update_subscription(&self, subscription: &SubscriptionSource) -> Result<()>;

    /// Delete subscription
    async fn delete_subscription(&self, tenant_id: &str, id: uuid::Uuid) -> Result<()>;

    /// List all subscriptions for a tenant
    async fn list_subscriptions(&self, tenant_id: &str) -> Result<Vec<SubscriptionSource>>;

    /// List enabled subscriptions for a tenant
    async fn list_enabled_subscriptions(&self, tenant_id: &str) -> Result<Vec<SubscriptionSource>>;

    /// Update subscription status (last_run_at, last_run_status)
    async fn update_subscription_status(&self, id: uuid::Uuid, status: &str) -> Result<()>;

    /// Increment error count for a subscription
    async fn increment_error_count(&self, id: uuid::Uuid) -> Result<()>;

    /// Reset error count for a subscription
    async fn reset_error_count(&self, id: uuid::Uuid) -> Result<()>;
}

// Re-export implementations
pub use graph::FalkorStore;
pub use graph::InMemoryGraphStore;
pub use postgres::PostgresStore;
pub use prediction_history::{
    InMemoryPredictionStore, PredictionErrorRecord, PredictionHistoryStore, PredictionStats,
    WindowedStats,
};
pub use s3::{LocalStore, S3Store, S3StoreWithFallback};
pub use vector::{InMemoryVectorStore, PgVectorStore};

// GapStore trait and KnowledgeGapRecord are already exported above

pub struct InMemorySubscriptionStore {
    data: std::sync::RwLock<
        std::collections::HashMap<uuid::Uuid, crate::subscription::SubscriptionSource>,
    >,
}

impl InMemorySubscriptionStore {
    pub fn new() -> Self {
        Self {
            data: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

impl Default for InMemorySubscriptionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SubscriptionStore for InMemorySubscriptionStore {
    async fn create_subscription(&self, s: &SubscriptionSource) -> Result<uuid::Uuid> {
        let id = s.id;
        self.data
            .write()
            .map_err(|_| cogkos_core::CogKosError::Internal("lock poisoned".into()))?
            .insert(id, s.clone());
        Ok(id)
    }
    async fn get_subscription(&self, _: &str, id: uuid::Uuid) -> Result<SubscriptionSource> {
        let guard = self
            .data
            .read()
            .map_err(|_| cogkos_core::CogKosError::Internal("lock poisoned".into()))?;
        match guard.get(&id) {
            Some(s) => Ok(s.clone()),
            None => Err(cogkos_core::CogKosError::NotFound(
                "subscription not found".into(),
            )),
        }
    }
    async fn update_subscription(&self, s: &SubscriptionSource) -> Result<()> {
        self.data
            .write()
            .map_err(|_| cogkos_core::CogKosError::Internal("lock poisoned".into()))?
            .insert(s.id, s.clone());
        Ok(())
    }
    async fn delete_subscription(&self, _: &str, id: uuid::Uuid) -> Result<()> {
        self.data
            .write()
            .map_err(|_| cogkos_core::CogKosError::Internal("lock poisoned".into()))?
            .remove(&id);
        Ok(())
    }
    async fn list_subscriptions(&self, _: &str) -> Result<Vec<SubscriptionSource>> {
        let guard = self
            .data
            .read()
            .map_err(|_| cogkos_core::CogKosError::Internal("lock poisoned".into()))?;
        Ok(guard.values().cloned().collect())
    }
    async fn list_enabled_subscriptions(&self, _: &str) -> Result<Vec<SubscriptionSource>> {
        let guard = self
            .data
            .read()
            .map_err(|_| cogkos_core::CogKosError::Internal("lock poisoned".into()))?;
        Ok(guard.values().filter(|s| s.enabled).cloned().collect())
    }
    async fn update_subscription_status(&self, _: uuid::Uuid, _: &str) -> Result<()> {
        Ok(())
    }
    async fn increment_error_count(&self, _: uuid::Uuid) -> Result<()> {
        Ok(())
    }
    async fn reset_error_count(&self, _: uuid::Uuid) -> Result<()> {
        Ok(())
    }
}

/// In-memory auth store with default test key
pub struct InMemoryAuthStoreWithKey {
    #[allow(clippy::type_complexity)]
    keys: Arc<RwLock<HashMap<String, (String, Vec<String>)>>>,
}

impl InMemoryAuthStoreWithKey {
    pub fn new() -> Self {
        let store = Self {
            keys: Arc::new(RwLock::new(HashMap::new())),
        };
        // Add default test key - this is a hack for testing
        let keys_clone = store.keys.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(async {
                let mut keys = keys_clone.write().await;
                keys.insert(
                    "test-api-key-12345".to_string(),
                    (
                        "test-tenant".to_string(),
                        vec!["read".to_string(), "write".to_string()],
                    ),
                );
            });
        })
        .join()
        .ok();
        store
    }
}

impl Default for InMemoryAuthStoreWithKey {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AuthStore for InMemoryAuthStoreWithKey {
    async fn validate_api_key(&self, api_key: &str) -> Result<(String, Vec<String>)> {
        let keys = self.keys.read().await;
        keys.get(api_key)
            .cloned()
            .ok_or_else(|| cogkos_core::CogKosError::AccessDenied("Invalid API key".to_string()))
    }

    async fn create_api_key(&self, tenant_id: &str, permissions: Vec<String>) -> Result<String> {
        let mut keys = self.keys.write().await;
        let api_key = uuid::Uuid::new_v4().to_string();
        keys.insert(api_key.clone(), (tenant_id.to_string(), permissions));
        Ok(api_key)
    }

    async fn revoke_api_key(&self, _key_hash: &str) -> Result<()> {
        Ok(())
    }
}

/// No-op memory layer store for testing
pub struct NoopMemoryLayerStore;

#[async_trait]
impl MemoryLayerStore for NoopMemoryLayerStore {
    async fn list_claims_by_memory_layer(
        &self,
        _: &str,
        _: &str,
        _: Option<&str>,
        _: usize,
    ) -> Result<Vec<EpistemicClaim>> {
        Ok(vec![])
    }
    async fn count_claims_by_memory_layer(
        &self,
        _: &str,
        _: &str,
        _: Option<&str>,
    ) -> Result<usize> {
        Ok(0)
    }
    async fn gc_expired_memory_layer(&self, _: &str, _: &str, _: f64) -> Result<usize> {
        Ok(0)
    }
    async fn promote_memory_layer(&self, _: &str, _: &str, _: &str, _: u64) -> Result<usize> {
        Ok(0)
    }
}
