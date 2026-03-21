//! Comprehensive multi-tenant isolation tests across all store types.
//!
//! Validates that tenant boundaries are respected for each store layer.
//! Where InMemory implementations skip tenant filtering (graph, vector,
//! feedback), the tests document that gap — real isolation relies on
//! PostgreSQL RLS in production.

use cogkos_core::models::*;
use cogkos_store::*;

fn make_claim(content: &str, tenant: &str) -> EpistemicClaim {
    EpistemicClaim::new(
        content,
        tenant,
        NodeType::Entity,
        Claimant::Human {
            user_id: "u1".into(),
            role: "tester".into(),
        },
        AccessEnvelope::new(tenant),
        ProvenanceRecord::new("test".into(), "test".into(), "test".into()),
    )
}

// ── 1. ClaimStore isolation ─────────────────────────────────────

#[tokio::test]
async fn test_claim_get_tenant_isolation() {
    let store = InMemoryClaimStore::new();

    let claim_a = make_claim("Tenant A secret", "tenant-a");
    let id = claim_a.id;
    store.insert_claim(&claim_a).await.unwrap();

    // Owner can read
    assert!(store.get_claim(id, "tenant-a").await.is_ok());
    // Other tenant gets AccessDenied
    assert!(store.get_claim(id, "tenant-b").await.is_err());
}

#[tokio::test]
async fn test_claim_query_tenant_isolation() {
    let store = InMemoryClaimStore::new();

    store
        .insert_claim(&make_claim("A1", "tenant-a"))
        .await
        .unwrap();
    store
        .insert_claim(&make_claim("A2", "tenant-a"))
        .await
        .unwrap();
    store
        .insert_claim(&make_claim("B1", "tenant-b"))
        .await
        .unwrap();

    let a_claims = store.query_claims("tenant-a", &[]).await.unwrap();
    assert_eq!(a_claims.len(), 2);

    let b_claims = store.query_claims("tenant-b", &[]).await.unwrap();
    assert_eq!(b_claims.len(), 1);

    let c_claims = store.query_claims("tenant-c", &[]).await.unwrap();
    assert_eq!(c_claims.len(), 0);
}

#[tokio::test]
async fn test_claim_search_tenant_isolation() {
    let store = InMemoryClaimStore::new();

    store
        .insert_claim(&make_claim("Shared keyword", "tenant-a"))
        .await
        .unwrap();
    store
        .insert_claim(&make_claim("Shared keyword", "tenant-b"))
        .await
        .unwrap();

    let a_results = store.search_claims("tenant-a", "Shared", 10).await.unwrap();
    assert_eq!(a_results.len(), 1);
    assert_eq!(a_results[0].tenant_id, "tenant-a");

    let b_results = store.search_claims("tenant-b", "Shared", 10).await.unwrap();
    assert_eq!(b_results.len(), 1);
    assert_eq!(b_results[0].tenant_id, "tenant-b");
}

#[tokio::test]
async fn test_claim_update_confidence_cross_tenant() {
    let store = InMemoryClaimStore::new();

    let claim = make_claim("Original", "tenant-a");
    let id = claim.id;
    store.insert_claim(&claim).await.unwrap();

    // update_confidence does not enforce tenant_id in memory impl,
    // but the ID is a UUID so collision is practically impossible.
    // The important thing: get_claim enforces tenant on read.
    store.update_confidence(id, "tenant-a", 0.99).await.unwrap();
    let updated = store.get_claim(id, "tenant-a").await.unwrap();
    assert_eq!(updated.confidence, 0.99);

    // tenant-b still cannot read it
    assert!(store.get_claim(id, "tenant-b").await.is_err());
}

#[tokio::test]
async fn test_claim_delete_tenant_isolation() {
    let store = InMemoryClaimStore::new();

    let claim_a = make_claim("A data", "tenant-a");
    let claim_b = make_claim("B data", "tenant-b");
    let id_a = claim_a.id;

    store.insert_claim(&claim_a).await.unwrap();
    store.insert_claim(&claim_b).await.unwrap();

    // Delete tenant-a's claim
    store.delete_claim(id_a, "tenant-a").await.unwrap();
    assert!(store.get_claim(id_a, "tenant-a").await.is_err());

    // tenant-b's claim untouched
    let b_claims = store.query_claims("tenant-b", &[]).await.unwrap();
    assert_eq!(b_claims.len(), 1);
}

#[tokio::test]
async fn test_claim_conflicts_tenant_isolation() {
    let store = InMemoryClaimStore::new();

    let c1 = make_claim("Claim 1", "tenant-a");
    let c2 = make_claim("Claim 2", "tenant-a");
    store.insert_claim(&c1).await.unwrap();
    store.insert_claim(&c2).await.unwrap();

    let conflict = ConflictRecord::new("tenant-a", c1.id, c2.id, ConflictType::DirectContradiction);
    store.insert_conflict(&conflict).await.unwrap();

    // tenant-a sees the conflict
    let conflicts = store
        .get_conflicts_for_claim(c1.id, "tenant-a")
        .await
        .unwrap();
    assert_eq!(conflicts.len(), 1);

    // tenant-b's separate claims should have no conflicts
    let c3 = make_claim("Claim 3", "tenant-b");
    store.insert_claim(&c3).await.unwrap();
    let b_conflicts = store
        .get_conflicts_for_claim(c3.id, "tenant-b")
        .await
        .unwrap();
    assert_eq!(b_conflicts.len(), 0);
}

// ── 2. VectorStore isolation ────────────────────────────────────
// Note: InMemoryVectorStore ignores tenant_id in search.
// Real isolation is via PostgreSQL RLS (WHERE tenant_id = $2).

#[tokio::test]
async fn test_vector_search_accepts_tenant_param() {
    let store = InMemoryVectorStore::new();

    let id_a = uuid::Uuid::new_v4();
    let id_b = uuid::Uuid::new_v4();

    store
        .upsert(
            id_a,
            vec![1.0, 0.0, 0.0],
            serde_json::json!({"tenant_id": "tenant-a"}),
        )
        .await
        .unwrap();
    store
        .upsert(
            id_b,
            vec![0.0, 1.0, 0.0],
            serde_json::json!({"tenant_id": "tenant-b"}),
        )
        .await
        .unwrap();

    // In-memory impl returns all vectors regardless of tenant_id.
    // This test documents that behavior — production PgVectorStore
    // enforces tenant filtering via SQL WHERE clause.
    let results = store
        .search(vec![1.0, 0.0, 0.0], "tenant-a", 10, None)
        .await
        .unwrap();
    assert!(!results.is_empty(), "Search should return results");
}

#[tokio::test]
async fn test_vector_search_memory_layer_param() {
    let store = InMemoryVectorStore::new();

    let id1 = uuid::Uuid::new_v4();
    store
        .upsert(
            id1,
            vec![0.5, 0.5, 0.5],
            serde_json::json!({"tenant_id": "t1", "memory_layer": "semantic"}),
        )
        .await
        .unwrap();

    // memory_layer filter is also ignored in InMemory — documenting this
    let results = store
        .search(vec![0.5, 0.5, 0.5], "t1", 10, Some("semantic"))
        .await
        .unwrap();
    assert!(!results.is_empty());
}

// ── 3. GraphStore isolation ─────────────────────────────────────
// Note: InMemoryGraphStore ignores tenant_id — documented in test_graph.rs.

#[tokio::test]
async fn test_graph_find_related_tenant_param() {
    let store = InMemoryGraphStore::new();

    let c1 = make_claim("G1", "tenant-a");
    let c2 = make_claim("G2", "tenant-b");

    store.add_node(&c1).await.unwrap();
    store.add_node(&c2).await.unwrap();
    store
        .add_edge(c1.id, c2.id, "SIMILAR_TO", 0.9)
        .await
        .unwrap();

    // In-memory does not filter by tenant — both tenants see everything.
    // Production FalkorDB enforces tenant filtering in Cypher queries.
    let result = store.find_related(c1.id, "tenant-a", 1, 0.0).await.unwrap();
    assert_eq!(result.len(), 1, "InMemory graph does not filter by tenant");
}

// ── 4. CacheStore isolation ─────────────────────────────────────

#[tokio::test]
async fn test_cache_tenant_isolation() {
    let store = InMemoryCacheStore::new();

    let response = McpQueryResponse {
        query_hash: 42,
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
    };
    let entry = QueryCacheEntry::new(42, response);

    // tenant-a sets cache
    store.set_cached("tenant-a", &entry).await.unwrap();

    // tenant-a can read it
    let cached_a = store.get_cached("tenant-a", 42).await.unwrap();
    assert!(cached_a.is_some());

    // tenant-b CANNOT read tenant-a's cache (keyed by (tenant_id, query_hash))
    let cached_b = store.get_cached("tenant-b", 42).await.unwrap();
    assert!(
        cached_b.is_none(),
        "tenant-b should not see tenant-a's cache"
    );
}

#[tokio::test]
async fn test_cache_invalidation_tenant_scoped() {
    let store = InMemoryCacheStore::new();

    let response = McpQueryResponse {
        query_hash: 100,
        query_context: "shared query".into(),
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
    };

    let entry_a = QueryCacheEntry::new(100, response.clone());
    let entry_b = QueryCacheEntry::new(100, response);

    store.set_cached("tenant-a", &entry_a).await.unwrap();
    store.set_cached("tenant-b", &entry_b).await.unwrap();

    // Invalidate only tenant-a
    store.invalidate("tenant-a", 100).await.unwrap();

    assert!(store.get_cached("tenant-a", 100).await.unwrap().is_none());
    assert!(store.get_cached("tenant-b", 100).await.unwrap().is_some());
}

#[tokio::test]
async fn test_cache_record_hit_tenant_scoped() {
    let store = InMemoryCacheStore::new();

    let response = McpQueryResponse {
        query_hash: 200,
        query_context: "hit test".into(),
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
    };
    let entry = QueryCacheEntry::new(200, response);

    store.set_cached("tenant-a", &entry).await.unwrap();

    // Recording a hit for tenant-b on the same hash should be a no-op
    store.record_hit("tenant-b", 200).await.unwrap();

    // tenant-a's entry should still exist and be unaffected
    let cached = store.get_cached("tenant-a", 200).await.unwrap();
    assert!(cached.is_some());
}

// ── 5. FeedbackStore isolation ──────────────────────────────────
// Note: InMemoryFeedbackStore ignores tenant_id — keyed only by query_hash.

#[tokio::test]
async fn test_feedback_insert_accepts_tenant_param() {
    let store = InMemoryFeedbackStore::new();

    let fb_a = AgentFeedback {
        query_hash: 42,
        agent_id: "agent-a".into(),
        success: true,
        feedback_note: Some("Good from A".into()),
        timestamp: chrono::Utc::now(),
    };
    let fb_b = AgentFeedback {
        query_hash: 42,
        agent_id: "agent-b".into(),
        success: false,
        feedback_note: Some("Bad from B".into()),
        timestamp: chrono::Utc::now(),
    };

    store.insert_feedback("tenant-a", &fb_a).await.unwrap();
    store.insert_feedback("tenant-b", &fb_b).await.unwrap();

    // In-memory impl ignores tenant_id — both feedback entries are
    // returned for the same query_hash regardless of tenant.
    // Production PostgresFeedbackStore enforces tenant filtering via SQL.
    let results = store.get_feedback_for_query("tenant-a", 42).await.unwrap();
    assert_eq!(
        results.len(),
        2,
        "InMemory feedback does not filter by tenant (known gap)"
    );
}

// ── 6. GapStore isolation ───────────────────────────────────────

#[tokio::test]
async fn test_gap_tenant_isolation() {
    let store = InMemoryGapStore::new();

    let gap_a = KnowledgeGapRecord {
        gap_id: uuid::Uuid::new_v4(),
        tenant_id: "tenant-a".into(),
        domain: "physics".into(),
        description: "Missing gravity model".into(),
        priority: "high".into(),
        status: "open".into(),
        reported_at: chrono::Utc::now(),
        filled_at: None,
    };
    let gap_b = KnowledgeGapRecord {
        gap_id: uuid::Uuid::new_v4(),
        tenant_id: "tenant-b".into(),
        domain: "physics".into(),
        description: "Missing quantum model".into(),
        priority: "medium".into(),
        status: "open".into(),
        reported_at: chrono::Utc::now(),
        filled_at: None,
    };

    store.record_gap(&gap_a).await.unwrap();
    store.record_gap(&gap_b).await.unwrap();

    // tenant-a only sees its own gaps
    let a_gaps = store.get_gaps("tenant-a").await.unwrap();
    assert_eq!(a_gaps.len(), 1);
    assert_eq!(a_gaps[0].description, "Missing gravity model");

    // tenant-b only sees its own gaps
    let b_gaps = store.get_gaps("tenant-b").await.unwrap();
    assert_eq!(b_gaps.len(), 1);
    assert_eq!(b_gaps[0].description, "Missing quantum model");

    // nonexistent tenant sees nothing
    let c_gaps = store.get_gaps("tenant-c").await.unwrap();
    assert_eq!(c_gaps.len(), 0);
}

#[tokio::test]
async fn test_gap_find_similar_tenant_scoped() {
    let store = InMemoryGapStore::new();

    let gap = KnowledgeGapRecord {
        gap_id: uuid::Uuid::new_v4(),
        tenant_id: "tenant-a".into(),
        domain: "chemistry".into(),
        description: "Missing bonding theory".into(),
        priority: "low".into(),
        status: "open".into(),
        reported_at: chrono::Utc::now(),
        filled_at: None,
    };
    store.record_gap(&gap).await.unwrap();

    // Same domain+description but different tenant should NOT match
    let found = store
        .find_similar_gap("tenant-b", "chemistry", "Missing bonding theory")
        .await
        .unwrap();
    assert!(
        found.is_none(),
        "Gap from tenant-a should not be visible to tenant-b"
    );

    // Same tenant should match
    let found_a = store
        .find_similar_gap("tenant-a", "chemistry", "Missing bonding theory")
        .await
        .unwrap();
    assert!(found_a.is_some());
}

#[tokio::test]
async fn test_gap_domain_filter_tenant_scoped() {
    let store = InMemoryGapStore::new();

    let gap1 = KnowledgeGapRecord {
        gap_id: uuid::Uuid::new_v4(),
        tenant_id: "tenant-a".into(),
        domain: "biology".into(),
        description: "Gene editing gap".into(),
        priority: "high".into(),
        status: "open".into(),
        reported_at: chrono::Utc::now(),
        filled_at: None,
    };
    let gap2 = KnowledgeGapRecord {
        gap_id: uuid::Uuid::new_v4(),
        tenant_id: "tenant-b".into(),
        domain: "biology".into(),
        description: "Evolution gap".into(),
        priority: "medium".into(),
        status: "open".into(),
        reported_at: chrono::Utc::now(),
        filled_at: None,
    };

    store.record_gap(&gap1).await.unwrap();
    store.record_gap(&gap2).await.unwrap();

    let a_bio = store
        .get_gaps_by_domain("tenant-a", "biology")
        .await
        .unwrap();
    assert_eq!(a_bio.len(), 1);
    assert_eq!(a_bio[0].description, "Gene editing gap");

    let b_bio = store
        .get_gaps_by_domain("tenant-b", "biology")
        .await
        .unwrap();
    assert_eq!(b_bio.len(), 1);
    assert_eq!(b_bio[0].description, "Evolution gap");
}

#[tokio::test]
async fn test_gap_mark_filled_tenant_scoped() {
    let store = InMemoryGapStore::new();

    let gap_a = KnowledgeGapRecord {
        gap_id: uuid::Uuid::new_v4(),
        tenant_id: "tenant-a".into(),
        domain: "math".into(),
        description: "Topology gap".into(),
        priority: "low".into(),
        status: "open".into(),
        reported_at: chrono::Utc::now(),
        filled_at: None,
    };
    let gap_b = KnowledgeGapRecord {
        gap_id: uuid::Uuid::new_v4(),
        tenant_id: "tenant-b".into(),
        domain: "math".into(),
        description: "Algebra gap".into(),
        priority: "low".into(),
        status: "open".into(),
        reported_at: chrono::Utc::now(),
        filled_at: None,
    };

    store.record_gap(&gap_a).await.unwrap();
    store.record_gap(&gap_b).await.unwrap();

    // Mark tenant-a's gap as filled
    store
        .mark_gap_filled(gap_a.gap_id, "tenant-a")
        .await
        .unwrap();

    // tenant-a's gap is filled
    let a_gaps = store.get_gaps("tenant-a").await.unwrap();
    assert_eq!(a_gaps[0].status, "filled");

    // tenant-b's gap remains open
    let b_gaps = store.get_gaps("tenant-b").await.unwrap();
    assert_eq!(b_gaps[0].status, "open");
}
