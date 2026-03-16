//! Basic integration tests using in-memory stores

use cogkos_core::audit::InMemoryAuditStore;
use cogkos_store::*;
use cogkos_core::models::*;
use std::sync::Arc;

/// Minimal in-memory object store for testing
struct MockObjectStore;

#[async_trait::async_trait]
impl ObjectStore for MockObjectStore {
    async fn upload(&self, _key: &str, _data: &[u8], _content_type: &str) -> cogkos_core::Result<String> {
        Ok("mock://uploaded".into())
    }
    async fn download(&self, _key: &str) -> cogkos_core::Result<Vec<u8>> {
        Ok(vec![])
    }
    async fn delete(&self, _key: &str) -> cogkos_core::Result<()> {
        Ok(())
    }
    async fn presigned_url(&self, _key: &str, _expiry_secs: u64) -> cogkos_core::Result<String> {
        Ok("mock://url".into())
    }
}

async fn create_stores() -> Stores {
    let claims: Arc<dyn ClaimStore> = Arc::new(InMemoryClaimStore::new());
    let vectors: Arc<dyn VectorStore> = Arc::new(InMemoryVectorStore::new());
    let graph: Arc<dyn GraphStore> = Arc::new(InMemoryGraphStore::new());
    let cache: Arc<dyn CacheStore> = Arc::new(InMemoryCacheStore::new());
    let feedback: Arc<dyn FeedbackStore> = Arc::new(InMemoryFeedbackStore::new());
    let objects: Arc<dyn ObjectStore> = Arc::new(MockObjectStore);
    let auth: Arc<dyn AuthStore> = Arc::new(InMemoryAuthStore::new());
    let gaps: Arc<dyn GapStore> = Arc::new(InMemoryGapStore::new());
    let audit: Arc<dyn AuditStore> = Arc::new(InMemoryAuditStore::new(1000));
    let subscription: Arc<dyn SubscriptionStore> = Arc::new(InMemorySubscriptionStore::new());

    Stores::new(claims, vectors, graph, cache, feedback, objects, auth, gaps, audit, subscription)
}

fn make_claim(content: &str, tenant: &str) -> EpistemicClaim {
    EpistemicClaim::new(
        content, tenant, NodeType::Entity,
        Claimant::Human { user_id: "u1".into(), role: "tester".into() },
        AccessEnvelope::new("t1"), ProvenanceRecord::new("test".into(), "test".into(), "test".into()),
    )
}

#[tokio::test]
async fn test_claim_insert_and_get() {
    let stores = create_stores().await;
    let claim = make_claim("Earth is round", "tenant-1");
    let id = claim.id;

    let inserted_id = stores.claims.insert_claim(&claim).await.unwrap();
    assert_eq!(inserted_id, id);

    let retrieved = stores.claims.get_claim(id, "tenant-1").await.unwrap();
    assert_eq!(retrieved.content, "Earth is round");
}

#[tokio::test]
async fn test_claim_update() {
    let stores = create_stores().await;
    let mut claim = make_claim("Initial", "t1");
    let id = claim.id;
    stores.claims.insert_claim(&claim).await.unwrap();

    claim.content = "Updated".into();
    claim.confidence = 0.9;
    stores.claims.update_claim(&claim).await.unwrap();

    let r = stores.claims.get_claim(id, "t1").await.unwrap();
    assert_eq!(r.content, "Updated");
    assert_eq!(r.confidence, 0.9);
}

#[tokio::test]
async fn test_claim_delete() {
    let stores = create_stores().await;
    let claim = make_claim("To be deleted", "t1");
    let id = claim.id;
    stores.claims.insert_claim(&claim).await.unwrap();
    stores.claims.delete_claim(id, "t1").await.unwrap();
    assert!(stores.claims.get_claim(id, "t1").await.is_err());
}

#[tokio::test]
async fn test_vector_upsert_and_search() {
    let stores = create_stores().await;
    let id = uuid::Uuid::new_v4();
    let vector = vec![0.1, 0.2, 0.3, 0.4];
    stores.vectors.upsert(id, vector.clone(), serde_json::json!({"tenant_id": "t1"})).await.unwrap();
    let results = stores.vectors.search(vector, "t1", 10).await.unwrap();
    assert!(!results.is_empty());
}

#[tokio::test]
async fn test_graph_add_and_find() {
    let stores = create_stores().await;
    let c1 = make_claim("Node A", "t1");
    let c2 = make_claim("Node B", "t1");
    stores.graph.add_node(&c1).await.unwrap();
    stores.graph.add_node(&c2).await.unwrap();
    stores.graph.add_edge(c1.id, c2.id, "CAUSES", 0.8).await.unwrap();
    let related = stores.graph.find_related(c1.id, 1, 0.0).await.unwrap();
    assert!(!related.is_empty());
}

#[tokio::test]
async fn test_cache_set_and_get() {
    let stores = create_stores().await;
    let response = McpQueryResponse {
        query_hash: 12345,
        query_context: "test query".into(),
        best_belief: None,
        related_by_graph: vec![],
        conflicts: vec![],
        prediction: None,
        knowledge_gaps: vec![],
        freshness: FreshnessInfo { newest_source: None, oldest_source: None, staleness_warning: false },
        cache_status: CacheStatus::Miss,
        metadata: QueryMetadata::default(),
    };
    let entry = QueryCacheEntry::new(12345, response);
    stores.cache.set_cached("t1", &entry).await.unwrap();
    let cached = stores.cache.get_cached("t1", 12345).await.unwrap();
    assert!(cached.is_some());
    assert_eq!(cached.unwrap().query_hash, 12345);
}

#[tokio::test]
async fn test_gap_record_and_get() {
    let stores = create_stores().await;
    let gap = KnowledgeGapRecord {
        gap_id: uuid::Uuid::new_v4(),
        tenant_id: "t1".into(),
        domain: "physics".into(),
        description: "Missing gravity theory".into(),
        priority: "high".into(),
        status: "open".into(),
        reported_at: chrono::Utc::now(),
        filled_at: None,
    };
    stores.gaps.record_gap(&gap).await.unwrap();
    let gaps = stores.gaps.get_gaps("t1").await.unwrap();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].domain, "physics");
}

#[tokio::test]
async fn test_auth_create_and_validate() {
    let stores = create_stores().await;
    let key = stores.auth.create_api_key("t1", vec!["read".into(), "write".into()]).await.unwrap();
    let (tenant, perms) = stores.auth.validate_api_key(&key).await.unwrap();
    assert_eq!(tenant, "t1");
    assert!(perms.contains(&"read".to_string()));
}

#[tokio::test]
async fn test_conflict_resolution_flow() {
    let stores = create_stores().await;

    // Insert two conflicting claims
    let c1 = make_claim("Product X is excellent", "t1");
    let c2 = make_claim("Product X is terrible", "t1");
    stores.claims.insert_claim(&c1).await.unwrap();
    stores.claims.insert_claim(&c2).await.unwrap();

    // Create a conflict record
    let mut conflict = ConflictRecord::new(
        "t1",
        c1.id,
        c2.id,
        ConflictType::DirectContradiction,
    );
    conflict.severity = 0.85;
    let conflict_id = conflict.id;
    stores.claims.insert_conflict(&conflict).await.unwrap();

    // Verify conflict exists as Open
    let conflicts = stores.claims.get_conflicts_for_claim(c1.id).await.unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].resolution_status, ResolutionStatus::Open);

    // Resolve the conflict
    stores
        .claims
        .resolve_conflict(conflict_id, ResolutionStatus::Accepted, Some("Both claims valid in different contexts".into()))
        .await
        .unwrap();

    // Verify resolution persisted
    let conflicts = stores.claims.get_conflicts_for_claim(c1.id).await.unwrap();
    assert_eq!(conflicts[0].resolution_status, ResolutionStatus::Accepted);
    assert!(conflicts[0].resolved_at.is_some());
    assert_eq!(conflicts[0].resolution_note.as_deref(), Some("Both claims valid in different contexts"));
}

#[tokio::test]
async fn test_ingest_store_query_flow() {
    let stores = create_stores().await;

    // Step 1: Ingest — create claims and store with vectors
    let claim1 = make_claim("Rust provides memory safety without garbage collection", "t1");
    let claim2 = make_claim("Rust uses ownership and borrowing for memory management", "t1");

    stores.claims.insert_claim(&claim1).await.unwrap();
    stores.claims.insert_claim(&claim2).await.unwrap();

    // Step 2: Store vectors (simulated embeddings)
    let v1 = vec![0.9, 0.1, 0.3, 0.5];
    let v2 = vec![0.85, 0.15, 0.35, 0.45];
    stores.vectors.upsert(claim1.id, v1.clone(), serde_json::json!({"tenant_id": "t1", "content": &claim1.content})).await.unwrap();
    stores.vectors.upsert(claim2.id, v2.clone(), serde_json::json!({"tenant_id": "t1", "content": &claim2.content})).await.unwrap();

    // Step 3: Store graph relationships
    stores.graph.add_node(&claim1).await.unwrap();
    stores.graph.add_node(&claim2).await.unwrap();
    stores.graph.add_edge(claim1.id, claim2.id, "RELATED_TO", 0.9).await.unwrap();

    // Step 4: Query — vector search should find both claims
    let query_vec = vec![0.88, 0.12, 0.32, 0.48]; // similar to both
    let results = stores.vectors.search(query_vec, "t1", 10).await.unwrap();
    assert!(results.len() >= 2, "Expected at least 2 vector matches, got {}", results.len());

    // Step 5: Verify claims retrievable by ID
    let retrieved = stores.claims.get_claim(claim1.id, "t1").await.unwrap();
    assert_eq!(retrieved.content, "Rust provides memory safety without garbage collection");

    // Step 6: Graph traversal finds related claims
    let related = stores.graph.find_related(claim1.id, 1, 0.0).await.unwrap();
    assert!(!related.is_empty(), "Graph should find related nodes");

    // Step 7: Cache round-trip
    let response = McpQueryResponse {
        query_hash: 99999,
        query_context: "Rust memory safety".into(),
        best_belief: None,
        related_by_graph: vec![],
        conflicts: vec![],
        prediction: None,
        knowledge_gaps: vec![],
        freshness: FreshnessInfo { newest_source: None, oldest_source: None, staleness_warning: false },
        cache_status: CacheStatus::Miss,
        metadata: QueryMetadata::default(),
    };
    let entry = QueryCacheEntry::new(99999, response);
    stores.cache.set_cached("t1", &entry).await.unwrap();
    let cached = stores.cache.get_cached("t1", 99999).await.unwrap();
    assert!(cached.is_some());
}
