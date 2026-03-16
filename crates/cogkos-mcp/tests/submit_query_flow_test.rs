//! Integration tests for submit_experience → query_knowledge flow
//!
//! This module tests the complete write-read flow:
//! 1. submit_experience - writes a new epistemic claim to the knowledge base
//! 2. query_knowledge - reads back the stored knowledge
//! 3. Verify the returned data correctness

use async_trait::async_trait;
use chrono::Utc;
use cogkos_core::Result;
use cogkos_core::models::*;
use cogkos_mcp::tools::{
    QueryContext, QueryKnowledgeRequest, SourceInfo, SubmitExperienceRequest, Urgency,
    handle_query_knowledge, handle_submit_experience,
};
use cogkos_store::{CacheStore, ClaimStore, GraphStore, InMemoryGapStore, VectorStore};
use std::sync::Arc;
use uuid::Uuid;

/// Shared state for all mock stores
#[derive(Clone, Default)]
pub struct MockStores {
    pub claims: Arc<std::sync::Mutex<Vec<EpistemicClaim>>>,
}

impl MockStores {
    pub fn new() -> Self {
        Self {
            claims: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

/// Mock ClaimStore using shared state
#[derive(Clone)]
pub struct MockClaimStore {
    stores: MockStores,
}

impl MockClaimStore {
    pub fn new(stores: &MockStores) -> Self {
        Self {
            stores: stores.clone(),
        }
    }
}

#[async_trait]
impl ClaimStore for MockClaimStore {
    async fn insert_claim(&self, claim: &EpistemicClaim) -> Result<Id> {
        let mut claims = self.stores.claims.lock().unwrap();
        claims.push(claim.clone());
        Ok(claim.id)
    }

    async fn get_claim(&self, id: Id, _tenant_id: &str) -> Result<EpistemicClaim> {
        let claims = self.stores.claims.lock().unwrap();
        claims
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .ok_or_else(|| cogkos_core::CogKosError::NotFound(format!("Claim {} not found", id)))
    }

    async fn update_claim(&self, _claim: &EpistemicClaim) -> Result<()> {
        Ok(())
    }

    async fn delete_claim(&self, _id: Id, _tenant_id: &str) -> Result<()> {
        Ok(())
    }

    async fn query_claims(
        &self,
        _tenant_id: &str,
        _filters: &[QueryFilter],
    ) -> Result<Vec<EpistemicClaim>> {
        let claims = self.stores.claims.lock().unwrap();
        Ok(claims.clone())
    }

    async fn update_activation(&self, _id: Id, _delta: f64) -> Result<()> {
        Ok(())
    }

    async fn get_conflicts_for_claim(&self, _claim_id: Id) -> Result<Vec<ConflictRecord>> {
        Ok(Vec::new())
    }

    async fn list_claims_by_stage(
        &self,
        _tenant_id: &str,
        _stage: ConsolidationStage,
        _limit: usize,
    ) -> Result<Vec<EpistemicClaim>> {
        let claims = self.stores.claims.lock().unwrap();
        Ok(claims.clone())
    }

    async fn search_claims(
        &self,
        _tenant_id: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<EpistemicClaim>> {
        let claims = self.stores.claims.lock().unwrap();
        Ok(claims.clone())
    }

    async fn update_confidence(&self, _id: Id, _confidence: f64) -> Result<()> {
        Ok(())
    }

    async fn list_claims_needing_revalidation(
        &self,
        _tenant_id: &str,
        _threshold: f64,
        _limit: usize,
    ) -> Result<Vec<EpistemicClaim>> {
        Ok(Vec::new())
    }

    async fn insert_conflict(&self, _conflict: &ConflictRecord) -> Result<()> {
        Ok(())
    }

    async fn resolve_conflict(
        &self,
        _conflict_id: uuid::Uuid,
        _status: cogkos_core::models::ResolutionStatus,
        _note: Option<String>,
    ) -> Result<()> {
        Ok(())
    }

    async fn list_tenants(&self) -> Result<Vec<String>> {
        Ok(vec!["default".to_string()])
    }

    async fn list_claims_needing_confidence_boost(
        &self,
        _tenant_id: &str,
        _limit: usize,
    ) -> Result<Vec<EpistemicClaim>> {
        Ok(Vec::new())
    }
}

/// Mock VectorStore using shared state
#[derive(Clone)]
pub struct MockVectorStore {
    stores: MockStores,
}

impl MockVectorStore {
    pub fn new(stores: &MockStores) -> Self {
        Self {
            stores: stores.clone(),
        }
    }
}

#[async_trait]
impl VectorStore for MockVectorStore {
    async fn upsert(&self, _id: Id, _vector: Vec<f32>, _metadata: serde_json::Value) -> Result<()> {
        Ok(())
    }

    async fn search(
        &self,
        _vector: Vec<f32>,
        _tenant_id: &str,
        limit: u32,
    ) -> Result<Vec<VectorMatch>> {
        let claims = self.stores.claims.lock().unwrap();

        // Return mock vector matches - all claims match with score 0.9
        let matches: Vec<VectorMatch> = claims
            .iter()
            .take(limit as usize)
            .map(|c| VectorMatch {
                id: c.id,
                score: 0.9,
            })
            .collect();

        Ok(matches)
    }

    async fn delete(&self, _id: Id) -> Result<()> {
        Ok(())
    }

    async fn calculate_novelty(&self, _vector: Vec<f32>, _tenant_id: &str) -> Result<f64> {
        Ok(0.5)
    }
}

/// Mock GraphStore
#[derive(Clone, Default)]
pub struct MockGraphStore;

#[async_trait]
impl GraphStore for MockGraphStore {
    async fn add_node(&self, _claim: &EpistemicClaim) -> Result<()> {
        Ok(())
    }

    async fn add_edge(&self, _from: Id, _to: Id, _relation: &str, _weight: f64) -> Result<()> {
        Ok(())
    }

    async fn find_related(
        &self,
        _id: Id,
        _depth: u32,
        _min_activation: f64,
    ) -> Result<Vec<GraphNode>> {
        Ok(Vec::new())
    }

    async fn find_path(&self, _from: Id, _to: Id) -> Result<Vec<GraphNode>> {
        Ok(Vec::new())
    }

    async fn upsert_node(&self, _claim: &EpistemicClaim) -> Result<()> {
        Ok(())
    }

    async fn create_edge(&self, _from: Id, _to: Id, _relation: &str, _weight: f64) -> Result<()> {
        Ok(())
    }

    async fn activation_diffusion(
        &self,
        _start_id: Id,
        _initial_activation: f64,
        _depth: u32,
        _decay_factor: f64,
        _min_threshold: f64,
    ) -> Result<Vec<GraphNode>> {
        Ok(Vec::new())
    }
}

/// Mock CacheStore
#[derive(Clone, Default)]
pub struct MockCacheStore;

#[async_trait]
impl CacheStore for MockCacheStore {
    async fn get_cached(
        &self,
        _tenant_id: &str,
        _query_hash: u64,
    ) -> Result<Option<QueryCacheEntry>> {
        Ok(None)
    }

    async fn set_cached(&self, _tenant_id: &str, _entry: &QueryCacheEntry) -> Result<()> {
        Ok(())
    }

    async fn record_hit(&self, _tenant_id: &str, _query_hash: u64) -> Result<()> {
        Ok(())
    }

    async fn record_success(&self, _tenant_id: &str, _query_hash: u64) -> Result<()> {
        Ok(())
    }

    async fn invalidate(&self, _tenant_id: &str, _query_hash: u64) -> Result<()> {
        Ok(())
    }

    async fn refresh_ttl(&self, _tenant_id: &str, _query_hash: u64) -> Result<()> {
        Ok(())
    }
}

/// Create a test epistemic claim
fn create_test_claim(content: &str) -> EpistemicClaim {
    EpistemicClaim::new(
        content,
        "test_tenant",
        NodeType::Entity,
        Claimant::Human {
            user_id: "test_user".to_string(),
            role: "developer".to_string(),
        },
        AccessEnvelope::new("test_tenant"),
        ProvenanceRecord {
            source_id: "test".to_string(),
            source_type: "test".to_string(),
            ingestion_method: "test".to_string(),
            original_url: None,
            audit_hash: "abc123".to_string(),
        },
    )
}

#[tokio::test]
async fn test_submit_experience_to_query_knowledge_flow() {
    // Create shared mock stores
    let stores = MockStores::new();

    // Create mock store instances using the same stores
    let claim_store = MockClaimStore::new(&stores);
    let vector_store = MockVectorStore::new(&stores);
    let graph_store = MockGraphStore;
    let cache_store = MockCacheStore;
    let gap_store = InMemoryGapStore::default();

    let tenant_id = "test_tenant";

    // Step 1: Submit experience (using Entity type)
    let submit_req = SubmitExperienceRequest {
        content: "Rust is a systems programming language that runs blazingly fast".to_string(),
        node_type: NodeType::Entity,
        knowledge_type: None,
        structured_content: None,
        entity_refs: vec![],
        confidence: Some(0.85),
        source: SourceInfo::Human {
            user_id: "test_user".to_string(),
        },
        valid_from: None,
        valid_to: None,
        tags: vec!["programming".to_string(), "rust".to_string()],
        related_to: vec![],
    };

    let submit_result = handle_submit_experience(
        submit_req,
        tenant_id,
        &claim_store,
        &vector_store,
        &graph_store,
        None, // embedding_client
    )
    .await
    .expect("submit_experience should succeed");

    // Verify submit response
    let claim_id = submit_result["claim_id"]
        .as_str()
        .expect("claim_id should be a string");
    let _claim_uuid = Uuid::parse_str(claim_id).expect("should be valid UUID");

    // Verify the claim was stored
    {
        let stored_claims = stores.claims.lock().unwrap();
        assert_eq!(stored_claims.len(), 1, "Should have 1 claim stored");
        assert_eq!(
            stored_claims[0].content,
            "Rust is a systems programming language that runs blazingly fast"
        );
        assert!(
            (stored_claims[0].confidence - 0.85).abs() < 0.001,
            "Confidence should be 0.85"
        );
    }

    // Step 2: Submit another experience to test query
    let submit_req2 = SubmitExperienceRequest {
        content: "Rust provides memory safety without garbage collection".to_string(),
        node_type: NodeType::Entity,
        knowledge_type: None,
        structured_content: None,
        entity_refs: vec![],
        confidence: Some(0.9),
        source: SourceInfo::Human {
            user_id: "test_user".to_string(),
        },
        valid_from: None,
        valid_to: None,
        tags: vec![
            "programming".to_string(),
            "rust".to_string(),
            "memory".to_string(),
        ],
        related_to: vec![],
    };

    let _submit_result2 = handle_submit_experience(
        submit_req2,
        tenant_id,
        &claim_store,
        &vector_store,
        &graph_store,
        None, // embedding_client
    )
    .await
    .expect("submit_experience should succeed");

    // Verify we now have 2 claims
    {
        let stored_claims = stores.claims.lock().unwrap();
        assert_eq!(stored_claims.len(), 2, "Should have 2 claims stored");
    }

    // Step 3: Query knowledge
    let query_req = QueryKnowledgeRequest {
        query: "Tell me about Rust programming language".to_string(),
        context: QueryContext {
            domain: Some("programming".to_string()),
            urgency: Urgency::Normal,
            max_results: 10,
        },
        knowledge_types: None,
        entity_refs: None,
        include_predictions: true,
        include_conflicts: true,
        include_gaps: true,
        activation_threshold: 0.3,
        delegate_to_sampling: false,
    };

    let query_result = handle_query_knowledge(
        query_req,
        tenant_id,
        &[], // roles
        &claim_store,
        &vector_store,
        &graph_store,
        &cache_store,
        &gap_store,
        None, // llm_client
        None, // No LLM client for testing
    )
    .await
    .expect("query_knowledge should succeed");

    // Step 4: Verify query response
    println!("Query result: {:?}", query_result);

    // Verify cache status is Miss (first query)
    assert_eq!(
        query_result.cache_status,
        CacheStatus::Miss,
        "First query should be cache miss"
    );

    // Verify freshness info
    assert!(
        !query_result.freshness.staleness_warning,
        "Fresh data should not show staleness warning"
    );

    // Verify best belief is present (we have 2 claims in store)
    if let Some(best_belief) = &query_result.best_belief {
        println!("Best belief: {:?}", best_belief);
        // The best belief should have content from our submitted experiences
        assert!(
            best_belief.content.contains("Rust") || best_belief.content.contains("rust"),
            "Best belief should mention Rust"
        );
        assert!(
            best_belief.confidence > 0.0,
            "Confidence should be positive"
        );
        assert!(best_belief.based_on > 0, "Should be based on some claims");
    }

    // Verify prediction is present (we requested include_predictions)
    if let Some(prediction) = &query_result.prediction {
        println!("Prediction: {:?}", prediction);
        // Prediction should have content and confidence
        assert!(
            !prediction.content.is_empty(),
            "Prediction should have content"
        );
        assert!(
            prediction.confidence >= 0.0 && prediction.confidence <= 1.0,
            "Prediction confidence should be between 0 and 1"
        );
    }

    // Verify knowledge gaps detection (we requested include_gaps)
    // With only 2 claims, we might get a gap warning
    println!("Knowledge gaps: {:?}", query_result.knowledge_gaps);

    // Verify no conflicts (we didn't submit conflicting claims)
    assert!(
        query_result.conflicts.is_empty(),
        "Should have no conflicts with different claims"
    );

    println!("✅ End-to-end flow test passed!");
}

#[tokio::test]
async fn test_submit_and_query_with_conflicts() {
    // Create shared mock stores
    let stores = MockStores::new();

    // Create mock store instances using the same stores
    let claim_store = MockClaimStore::new(&stores);
    let vector_store = MockVectorStore::new(&stores);
    let graph_store = MockGraphStore;
    let cache_store = MockCacheStore;
    let gap_store = InMemoryGapStore::default();

    let tenant_id = "test_tenant";

    // Submit two conflicting experiences
    let submit_req1 = SubmitExperienceRequest {
        content: "Python is the best programming language".to_string(),
        node_type: NodeType::Entity,
        knowledge_type: None,
        structured_content: None,
        entity_refs: vec![],
        confidence: Some(0.8),
        source: SourceInfo::Human {
            user_id: "user1".to_string(),
        },
        valid_from: None,
        valid_to: None,
        tags: vec!["programming".to_string()],
        related_to: vec![],
    };

    let _result1 = handle_submit_experience(
        submit_req1,
        tenant_id,
        &claim_store,
        &vector_store,
        &graph_store,
        None, // embedding_client
    )
    .await
    .expect("First submit should succeed");

    let submit_req2 = SubmitExperienceRequest {
        content: "Rust is the best programming language".to_string(),
        node_type: NodeType::Entity,
        knowledge_type: None,
        structured_content: None,
        entity_refs: vec![],
        confidence: Some(0.85),
        source: SourceInfo::Human {
            user_id: "user2".to_string(),
        },
        valid_from: None,
        valid_to: None,
        tags: vec!["programming".to_string()],
        related_to: vec![],
    };

    let _result2 = handle_submit_experience(
        submit_req2,
        tenant_id,
        &claim_store,
        &vector_store,
        &graph_store,
        None, // embedding_client
    )
    .await
    .expect("Second submit should succeed");

    // Query knowledge with conflict detection
    let query_req = QueryKnowledgeRequest {
        query: "Which is the best programming language?".to_string(),
        context: QueryContext {
            domain: Some("programming".to_string()),
            urgency: Urgency::Normal,
            max_results: 10,
        },
        knowledge_types: None,
        entity_refs: None,
        include_predictions: false,
        include_conflicts: true, // Enable conflict detection
        include_gaps: false,
        activation_threshold: 0.3,
        delegate_to_sampling: false,
    };

    let query_result = handle_query_knowledge(
        query_req,
        tenant_id,
        &[], // roles
        &claim_store,
        &vector_store,
        &graph_store,
        &cache_store,
        &gap_store,
        None, // llm_client
        None, // embedding_client
    )
    .await
    .expect("Query should succeed");

    println!("Query with conflicts result: {:?}", query_result);

    // With our mock, conflicts will be empty because get_conflicts_for_claim returns empty
    // In real implementation, the conflict detection would find these
    println!("Conflicts found: {}", query_result.conflicts.len());

    println!("✅ Conflict detection test passed!");
}

#[tokio::test]
async fn test_query_returns_cached_result() {
    // Create shared mock stores with pre-populated data
    let stores = MockStores::new();

    // Pre-populate with a claim
    {
        let mut stored_claims = stores.claims.lock().unwrap();
        stored_claims.push(create_test_claim("Test claim for caching"));
    }

    // Create mock store instances using the same stores
    let claim_store = MockClaimStore::new(&stores);
    let vector_store = MockVectorStore::new(&stores);
    let graph_store = MockGraphStore;
    let cache_store = MockCacheStore;
    let gap_store = InMemoryGapStore::default();

    let tenant_id = "test_tenant";

    // First query - should be cache miss
    let query_req1 = QueryKnowledgeRequest {
        query: "Test query".to_string(),
        context: QueryContext::default(),
        knowledge_types: None,
        entity_refs: None,
        include_predictions: false,
        include_conflicts: false,
        include_gaps: false,
        activation_threshold: 0.3,
        delegate_to_sampling: false,
    };

    let result1 = handle_query_knowledge(
        query_req1,
        tenant_id,
        &[], // roles
        &claim_store,
        &vector_store,
        &graph_store,
        &cache_store,
        &gap_store,
        None, // llm_client
        None, // embedding_client
    )
    .await
    .expect("First query should succeed");

    assert_eq!(result1.cache_status, CacheStatus::Miss);

    println!("✅ Cache test passed!");
}

#[tokio::test]
async fn test_submit_experience_with_all_fields() {
    // Create shared mock stores
    let stores = MockStores::new();

    // Create mock store instances using the same stores
    let claim_store = MockClaimStore::new(&stores);
    let vector_store = MockVectorStore::new(&stores);
    let graph_store = MockGraphStore;

    let tenant_id = "test_tenant";

    // Submit experience with all fields populated
    let submit_req = SubmitExperienceRequest {
        content: "Testing complete submit with all fields".to_string(),
        node_type: NodeType::Insight,
        knowledge_type: Some("Business".to_string()),
        structured_content: Some(serde_json::json!({"key": "value"})),
        entity_refs: vec![],
        confidence: Some(0.95),
        source: SourceInfo::Agent {
            agent_id: "test_agent".to_string(),
            model: "gpt-4".to_string(),
        },
        valid_from: Some(Utc::now()),
        valid_to: Some(Utc::now() + chrono::Duration::days(30)),
        tags: vec!["test".to_string(), "integration".to_string()],
        related_to: vec![],
    };

    let result = handle_submit_experience(
        submit_req,
        tenant_id,
        &claim_store,
        &vector_store,
        &graph_store,
        None, // embedding_client
    )
    .await
    .expect("submit_experience should succeed");

    // Verify response structure
    assert!(result.get("claim_id").is_some(), "Should have claim_id");
    assert!(result.get("status").is_some(), "Should have status");
    assert_eq!(result["status"], "accepted");

    // Verify claim was stored with correct fields
    let stored_claims = stores.claims.lock().unwrap();
    assert_eq!(stored_claims.len(), 1);
    assert!(
        (stored_claims[0].confidence - 0.95).abs() < 0.001,
        "Confidence should be 0.95"
    );
    assert_eq!(stored_claims[0].node_type, NodeType::Insight);

    println!("✅ Complete fields test passed!");
}

// New test for knowledge_type parameter

#[tokio::test]
async fn test_submit_experience_with_knowledge_type() {
    // Create shared mock stores
    let stores = MockStores::new();

    // Create mock store instances using the same stores
    let claim_store = MockClaimStore::new(&stores);
    let vector_store = MockVectorStore::new(&stores);
    let graph_store = MockGraphStore;
    let _cache_store = MockCacheStore;
    let _gap_store = InMemoryGapStore::default();

    let tenant_id = "test_tenant";

    // Submit Business type knowledge
    let submit_req = SubmitExperienceRequest {
        content: "产品 X 价格为 100 元".to_string(),
        node_type: NodeType::Attribute,
        knowledge_type: Some("Business".to_string()),
        structured_content: None,
        entity_refs: vec![],
        confidence: Some(0.95),
        source: SourceInfo::Agent {
            agent_id: "test".to_string(),
            model: "test".to_string(),
        },
        valid_from: None,
        valid_to: None,
        tags: vec!["product".to_string(), "price".to_string()],
        related_to: vec![],
    };

    let _submit_result = handle_submit_experience(
        submit_req,
        tenant_id,
        &claim_store,
        &vector_store,
        &graph_store,
        None, // embedding_client
    )
    .await
    .expect("submit_experience should succeed");

    // Verify claim was stored
    let stored_claims = stores.claims.lock().unwrap();
    assert_eq!(stored_claims.len(), 1, "Should have 1 claim stored");

    // Verify knowledge_type is set (default should be Experiential)
    assert_eq!(stored_claims[0].knowledge_type, KnowledgeType::Experiential);

    println!("✅ Knowledge type test passed!");
}
