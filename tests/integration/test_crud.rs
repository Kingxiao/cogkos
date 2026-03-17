//! CRUD integration tests for claim lifecycle

use cogkos_core::audit::InMemoryAuditStore;
use cogkos_core::models::*;
use cogkos_store::*;
use std::sync::Arc;

/// Minimal in-memory object store for testing
struct MockObjectStore;

#[async_trait::async_trait]
impl ObjectStore for MockObjectStore {
    async fn upload(
        &self,
        _key: &str,
        _data: &[u8],
        _content_type: &str,
    ) -> cogkos_core::Result<String> {
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
    Stores::new(
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
    )
}

fn make_claim(content: &str, tenant: &str) -> EpistemicClaim {
    EpistemicClaim::new(
        content,
        tenant,
        NodeType::Entity,
        Claimant::Human {
            user_id: "u1".into(),
            role: "tester".into(),
        },
        AccessEnvelope::new("t1"),
        ProvenanceRecord::new("test".into(), "test".into(), "test".into()),
    )
}

#[tokio::test]
async fn test_query_claims_by_tenant() {
    let stores = create_stores().await;
    let c1 = make_claim("Claim A", "t1");
    let c2 = make_claim("Claim B", "t1");
    let c3 = make_claim("Claim C", "t2");

    stores.claims.insert_claim(&c1).await.unwrap();
    stores.claims.insert_claim(&c2).await.unwrap();
    stores.claims.insert_claim(&c3).await.unwrap();

    let t1_claims = stores.claims.query_claims("t1", &[]).await.unwrap();
    assert_eq!(t1_claims.len(), 2);

    let t2_claims = stores.claims.query_claims("t2", &[]).await.unwrap();
    assert_eq!(t2_claims.len(), 1);
}

#[tokio::test]
async fn test_update_activation() {
    let stores = create_stores().await;
    let claim = make_claim("Test", "t1");
    let id = claim.id;
    stores.claims.insert_claim(&claim).await.unwrap();

    stores
        .claims
        .update_activation(id, "t1", 0.2)
        .await
        .unwrap();
    let r = stores.claims.get_claim(id, "t1").await.unwrap();
    assert!(r.activation_weight > 0.5); // default is 0.5, should increase
}

#[tokio::test]
async fn test_update_confidence() {
    let stores = create_stores().await;
    let claim = make_claim("Test", "t1");
    let id = claim.id;
    stores.claims.insert_claim(&claim).await.unwrap();

    stores
        .claims
        .update_confidence(id, "t1", 0.95)
        .await
        .unwrap();
    let r = stores.claims.get_claim(id, "t1").await.unwrap();
    assert_eq!(r.confidence, 0.95);
}

#[tokio::test]
async fn test_graph_activation_diffusion() {
    let stores = create_stores().await;
    let c1 = make_claim("Root", "t1");
    let c2 = make_claim("Level 1", "t1");
    let c3 = make_claim("Level 2", "t1");

    stores.graph.add_node(&c1).await.unwrap();
    stores.graph.add_node(&c2).await.unwrap();
    stores.graph.add_node(&c3).await.unwrap();
    stores
        .graph
        .add_edge(c1.id, c2.id, "CAUSES", 0.8)
        .await
        .unwrap();
    stores
        .graph
        .add_edge(c2.id, c3.id, "SIMILAR_TO", 0.6)
        .await
        .unwrap();

    let diffused = stores
        .graph
        .activation_diffusion(c1.id, 1.0, 3, 0.8, 0.01)
        .await
        .unwrap();

    // Should find at least c2 via activation diffusion
    assert!(!diffused.is_empty());
}

fn make_cache_response(hash: u64) -> McpQueryResponse {
    McpQueryResponse {
        query_hash: hash,
        query_context: "test".into(),
        best_belief: None,
        related_by_graph: vec![],
        conflicts: vec![],
        prediction: None,
        knowledge_gaps: vec![],
        freshness: FreshnessInfo {
            newest_source: None,
            oldest_source: None,
            staleness_warning: false,
        },
        cache_status: CacheStatus::Miss,
        cognitive_path: None,
        metadata: QueryMetadata::default(),
    }
}

#[tokio::test]
async fn test_cache_hit_recording() {
    let stores = create_stores().await;
    let entry = QueryCacheEntry::new(42, make_cache_response(42));
    stores.cache.set_cached("t1", &entry).await.unwrap();
    stores.cache.record_hit("t1", 42).await.unwrap();
    stores.cache.record_success("t1", 42).await.unwrap();

    let cached = stores.cache.get_cached("t1", 42).await.unwrap().unwrap();
    assert_eq!(cached.hit_count, 1);
    assert_eq!(cached.success_count, 1);
}

#[tokio::test]
async fn test_cache_invalidate() {
    let stores = create_stores().await;
    let entry = QueryCacheEntry::new(99, make_cache_response(99));
    stores.cache.set_cached("t1", &entry).await.unwrap();
    stores.cache.invalidate("t1", 99).await.unwrap();
    assert!(stores.cache.get_cached("t1", 99).await.unwrap().is_none());
}
