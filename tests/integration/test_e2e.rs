//! End-to-end integration tests: full knowledge lifecycle
//!
//! Validates the complete chain from ingestion through decay,
//! using InMemory stores only (no external services).

use cogkos_core::audit::InMemoryAuditStore;
use cogkos_core::evolution::decay::calculate_decay;
use cogkos_core::models::*;
use cogkos_store::*;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Helpers (mirrors test_basic.rs pattern)
// ---------------------------------------------------------------------------

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
    let memory_layers: Arc<dyn MemoryLayerStore> = Arc::new(NoopMemoryLayerStore);

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
        memory_layers,
    )
}

fn make_claim_with_confidence(content: &str, tenant: &str, confidence: f64) -> EpistemicClaim {
    let mut claim = EpistemicClaim::new(
        content,
        tenant,
        NodeType::Entity,
        Claimant::Human {
            user_id: "u1".into(),
            role: "tester".into(),
        },
        AccessEnvelope::new("t1"),
        ProvenanceRecord::new("test".into(), "test".into(), "test".into()),
    );
    claim.confidence = confidence;
    claim
}

// ---------------------------------------------------------------------------
// Test 1: Full knowledge lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_full_knowledge_lifecycle() {
    let stores = create_stores().await;
    let tenant = "lifecycle-tenant";

    // 1. Write 2 semantically related but conflicting claims
    let claim_a = make_claim_with_confidence(
        "Rust achieves memory safety through borrow checker",
        tenant,
        0.9,
    );
    let claim_b = make_claim_with_confidence(
        "Rust achieves memory safety through garbage collection",
        tenant,
        0.7,
    );
    let id_a = claim_a.id;
    let id_b = claim_b.id;

    stores.claims.insert_claim(&claim_a).await.unwrap();
    stores.claims.insert_claim(&claim_b).await.unwrap();

    // 2. Verify both retrievable
    let got_a = stores.claims.get_claim(id_a, tenant).await.unwrap();
    assert_eq!(got_a.confidence, 0.9);
    let got_b = stores.claims.get_claim(id_b, tenant).await.unwrap();
    assert_eq!(got_b.confidence, 0.7);

    // 3. Vector search: upsert embeddings then search
    let vec_a = vec![0.9, 0.1, 0.2, 0.8];
    let vec_b = vec![0.85, 0.15, 0.25, 0.75];
    stores
        .vectors
        .upsert(
            id_a,
            vec_a.clone(),
            serde_json::json!({"tenant_id": tenant}),
        )
        .await
        .unwrap();
    stores
        .vectors
        .upsert(
            id_b,
            vec_b.clone(),
            serde_json::json!({"tenant_id": tenant}),
        )
        .await
        .unwrap();

    let query_vec = vec![0.88, 0.12, 0.22, 0.78];
    let results = stores
        .vectors
        .search(query_vec, tenant, 10, None)
        .await
        .unwrap();
    assert!(
        results.len() >= 2,
        "Expected >=2 vector matches, got {}",
        results.len()
    );

    // 4. Graph: add nodes + edge, then find_related
    stores.graph.add_node(&claim_a).await.unwrap();
    stores.graph.add_node(&claim_b).await.unwrap();
    stores
        .graph
        .add_edge(id_a, id_b, "CONTRADICTS", 0.85)
        .await
        .unwrap();
    let related = stores
        .graph
        .find_related(id_a, tenant, 1, 0.0)
        .await
        .unwrap();
    assert!(!related.is_empty(), "Graph should find related node");

    // 5. Cache round-trip
    let response = McpQueryResponse {
        query_hash: 77777,
        query_context: "Rust memory safety".into(),
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
    let entry = QueryCacheEntry::new(77777, response);
    stores.cache.set_cached(tenant, &entry).await.unwrap();
    let cached = stores.cache.get_cached(tenant, 77777).await.unwrap();
    assert!(cached.is_some());
    assert_eq!(cached.unwrap().query_hash, 77777);

    // 6. Feedback round-trip
    let fb = AgentFeedback {
        query_hash: 77777,
        agent_id: "e2e-agent".into(),
        success: true,
        feedback_note: Some("accurate".into()),
        timestamp: chrono::Utc::now(),
    };
    stores.feedback.insert_feedback(tenant, &fb).await.unwrap();
    let fbs = stores
        .feedback
        .get_feedback_for_query(tenant, 77777)
        .await
        .unwrap();
    assert_eq!(fbs.len(), 1);
    assert!(fbs[0].success);

    // 7. Activation weight: update_activation increases weight
    let before = stores.claims.get_claim(id_a, tenant).await.unwrap();
    let original_weight = before.activation_weight;
    stores
        .claims
        .update_activation(id_a, tenant, 0.2)
        .await
        .unwrap();
    let after = stores.claims.get_claim(id_a, tenant).await.unwrap();
    assert!(
        after.activation_weight > original_weight,
        "activation_weight should increase: {} -> {}",
        original_weight,
        after.activation_weight,
    );
    assert_eq!(after.access_count, before.access_count + 1);

    // 8. Confidence update (writeback)
    stores
        .claims
        .update_confidence(id_b, tenant, 0.3)
        .await
        .unwrap();
    let updated_b = stores.claims.get_claim(id_b, tenant).await.unwrap();
    assert!(
        (updated_b.confidence - 0.3).abs() < f64::EPSILON,
        "confidence should be 0.3, got {}",
        updated_b.confidence,
    );

    // 9. Gap record
    let gap = KnowledgeGapRecord {
        gap_id: uuid::Uuid::new_v4(),
        tenant_id: tenant.into(),
        domain: "programming-languages".into(),
        description: "Missing comparison with linear types".into(),
        priority: "medium".into(),
        status: "open".into(),
        reported_at: chrono::Utc::now(),
        filled_at: None,
    };
    stores.gaps.record_gap(&gap).await.unwrap();
    let gaps = stores.gaps.get_gaps(tenant).await.unwrap();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].domain, "programming-languages");
}

// ---------------------------------------------------------------------------
// Test 2: Memory layer metadata tagging
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_memory_layer_metadata_tagging() {
    let stores = create_stores().await;
    let tenant = "layer-tenant";

    let layers = ["working", "episodic", "semantic"];
    let mut ids = Vec::new();

    for (i, layer) in layers.iter().enumerate() {
        let mut claim =
            make_claim_with_confidence(&format!("Claim in {} layer", layer), tenant, 0.8);
        claim.metadata.insert(
            "memory_layer".into(),
            serde_json::Value::String(layer.to_string()),
        );
        ids.push(claim.id);

        stores.claims.insert_claim(&claim).await.unwrap();

        // Upsert vector with memory_layer metadata
        let vec = vec![0.1 * (i as f32 + 1.0), 0.2, 0.3, 0.4];
        stores
            .vectors
            .upsert(
                claim.id,
                vec,
                serde_json::json!({"tenant_id": tenant, "memory_layer": layer}),
            )
            .await
            .unwrap();
    }

    // Verify claims stored with correct metadata
    for (i, layer) in layers.iter().enumerate() {
        let claim = stores.claims.get_claim(ids[i], tenant).await.unwrap();
        assert_eq!(
            claim.metadata.get("memory_layer").and_then(|v| v.as_str()),
            Some(*layer),
        );
    }

    // InMemoryVectorStore ignores memory_layer filter (documented behavior).
    // Verify search with None returns all 3.
    let query_vec = vec![0.15, 0.2, 0.3, 0.4];
    let all = stores
        .vectors
        .search(query_vec.clone(), tenant, 10, None)
        .await
        .unwrap();
    assert_eq!(all.len(), 3, "search(None) should return all 3 vectors");

    // With memory_layer=Some("semantic"), InMemory still returns all 3
    // (filtering only works in PostgreSQL). We document this limitation.
    let filtered = stores
        .vectors
        .search(query_vec, tenant, 10, Some("semantic"))
        .await
        .unwrap();
    assert_eq!(
        filtered.len(),
        3,
        "InMemory search ignores memory_layer filter (expected 3)"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Feedback → confidence writeback loop
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_feedback_confidence_writeback() {
    let stores = create_stores().await;
    let tenant = "feedback-tenant";

    // 1. Insert claim with confidence 0.8
    let claim = make_claim_with_confidence("TypeScript adds type safety to JS", tenant, 0.8);
    let claim_id = claim.id;
    stores.claims.insert_claim(&claim).await.unwrap();

    // 2. Create cache entry referencing this claim
    let best = BeliefSummary {
        claim_id: Some(claim_id),
        content: claim.content.clone(),
        confidence: 0.8,
        based_on: 1,
        consolidation_stage: ConsolidationStage::FastTrack,
        claim_ids: vec![claim_id],
        reliability: None,
    };
    let response = McpQueryResponse {
        query_hash: 55555,
        query_context: "TypeScript safety".into(),
        best_belief: Some(best),
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
    let entry = QueryCacheEntry::new(55555, response);
    stores.cache.set_cached(tenant, &entry).await.unwrap();

    // 3. Simulate feedback writeback: 70% original + 30% feedback
    //    Positive feedback (success=true) → feedback_confidence = 1.0
    //    Negative feedback (success=false) → feedback_confidence = 0.0
    let original = 0.8;
    let feedback_confidence = 1.0; // positive feedback
    let new_confidence = 0.7 * original + 0.3 * feedback_confidence;

    stores
        .claims
        .update_confidence(claim_id, tenant, new_confidence)
        .await
        .unwrap();

    // 4. Verify writeback
    let updated = stores.claims.get_claim(claim_id, tenant).await.unwrap();
    let expected = 0.7 * 0.8 + 0.3 * 1.0; // = 0.86
    assert!(
        (updated.confidence - expected).abs() < 1e-10,
        "Expected confidence ~{}, got {}",
        expected,
        updated.confidence,
    );

    // 5. Now simulate negative feedback on same claim
    let feedback_confidence_neg = 0.0;
    let new_conf_neg = 0.7 * updated.confidence + 0.3 * feedback_confidence_neg;
    stores
        .claims
        .update_confidence(claim_id, tenant, new_conf_neg)
        .await
        .unwrap();

    let after_neg = stores.claims.get_claim(claim_id, tenant).await.unwrap();
    assert!(
        after_neg.confidence < updated.confidence,
        "Negative feedback should lower confidence: {} -> {}",
        updated.confidence,
        after_neg.confidence,
    );
}

// ---------------------------------------------------------------------------
// Test 4: Knowledge decay verification
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_knowledge_decay() {
    let stores = create_stores().await;
    let tenant = "decay-tenant";

    // 1. Create claim with confidence=0.8
    let mut claim = make_claim_with_confidence("Microservices reduce coupling", tenant, 0.8);
    // Simulate created 30 days ago
    let thirty_days_ago = chrono::Utc::now() - chrono::Duration::days(30);
    claim.created_at = thirty_days_ago;
    let claim_id = claim.id;
    let original_confidence = claim.confidence;
    let activation_weight = claim.activation_weight;

    stores.claims.insert_claim(&claim).await.unwrap();

    // 2. Calculate decay using the core formula
    //    Default half_life = 720 hours (30 days)
    //    lambda = ln(2) / half_life_hours ≈ 0.000963
    let half_life_hours = 720.0;
    let lambda = (2.0_f64).ln() / half_life_hours;
    let time_delta_hours = 30.0 * 24.0; // 720 hours

    let decayed = calculate_decay(
        original_confidence,
        lambda,
        time_delta_hours,
        activation_weight,
    );

    // 3. Verify decay
    assert!(
        decayed < original_confidence,
        "Decayed confidence ({}) should be less than original ({})",
        decayed,
        original_confidence,
    );

    // After exactly 1 half-life with default activation_weight=0.5,
    // effective_lambda = lambda / max(0.5, 0.01) = lambda / 0.5 = 2*lambda
    // decay = 0.8 * exp(-2*lambda * 720) = 0.8 * exp(-2*ln(2)) = 0.8 * 0.25 = 0.2
    let expected_approx = 0.8 * (-2.0 * (2.0_f64).ln()).exp();
    assert!(
        (decayed - expected_approx).abs() < 1e-10,
        "Expected ~{}, got {}",
        expected_approx,
        decayed,
    );

    // 4. Write back decayed confidence to store
    stores
        .claims
        .update_confidence(claim_id, tenant, decayed)
        .await
        .unwrap();

    let updated = stores.claims.get_claim(claim_id, tenant).await.unwrap();
    assert!(
        (updated.confidence - decayed).abs() < f64::EPSILON,
        "Stored confidence should match decayed value",
    );

    // 5. Higher activation_weight should slow decay
    let high_activation_decay = calculate_decay(
        original_confidence,
        lambda,
        time_delta_hours,
        1.0, // max activation
    );
    assert!(
        high_activation_decay > decayed,
        "Higher activation ({}) should decay slower than default ({})",
        high_activation_decay,
        decayed,
    );
}

// ---------------------------------------------------------------------------
// Test 5: Authority tier resolution and integration
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_authority_tier_claim_lifecycle() {
    use cogkos_core::authority::AuthorityTier;

    let stores = create_stores().await;
    let tenant = "authority-tenant";

    // 1. Business + admin claim → Canonical
    let mut canonical =
        make_claim_with_confidence("Company policy: all data encrypted at rest", tenant, 0.95);
    canonical.knowledge_type = KnowledgeType::Business;
    canonical.claimant = Claimant::Human {
        user_id: "admin-1".into(),
        role: "admin".into(),
    };
    let tier = AuthorityTier::resolve(&canonical);
    assert_eq!(tier, AuthorityTier::Canonical);
    canonical.durability = tier.recommended_durability();
    canonical.metadata.insert(
        "authority_tier".into(),
        serde_json::Value::String(tier.as_str().to_string()),
    );
    stores.claims.insert_claim(&canonical).await.unwrap();

    // 2. Agent claim with default fields → Observed
    let agent_claim = make_claim_with_confidence("User prefers dark mode", tenant, 0.6);
    let agent_tier = AuthorityTier::resolve(&agent_claim);
    assert_eq!(agent_tier, AuthorityTier::Observed);
    stores.claims.insert_claim(&agent_claim).await.unwrap();

    // 3. Verify stored canonical claim retains metadata
    let stored = stores.claims.get_claim(canonical.id, tenant).await.unwrap();
    assert_eq!(
        stored
            .metadata
            .get("authority_tier")
            .and_then(|v| v.as_str()),
        Some("canonical"),
    );
    assert!((stored.durability - 2.0).abs() < f64::EPSILON);

    // 4. Canonical claim should not decay (multiplier = 0.0)
    assert_eq!(AuthorityTier::Canonical.decay_multiplier(), 0.0);

    // 5. Ephemeral claim decays faster
    let mut ephemeral = make_claim_with_confidence("temp working memory", tenant, 0.5);
    ephemeral.metadata.insert(
        "memory_layer".into(),
        serde_json::Value::String("working".into()),
    );
    let eph_tier = AuthorityTier::resolve(&ephemeral);
    assert_eq!(eph_tier, AuthorityTier::Ephemeral);
    assert!(eph_tier.decay_multiplier() > AuthorityTier::Observed.decay_multiplier());

    // 6. Score boost ordering in query ranking
    assert!(AuthorityTier::Canonical.score_boost() > AuthorityTier::Observed.score_boost(),);
    assert!(AuthorityTier::Observed.score_boost() > AuthorityTier::Ephemeral.score_boost(),);
}

#[tokio::test]
async fn test_authority_tier_conflict_suggestion() {
    use cogkos_core::authority::AuthorityTier;
    use cogkos_core::evolution::conflict::detect_conflict;

    let tenant = "conflict-authority";

    // High-authority Business claim
    let mut business_claim =
        make_claim_with_confidence("The product price is not 200 dollars", tenant, 0.9);
    business_claim.knowledge_type = KnowledgeType::Business;
    business_claim.claimant = Claimant::Human {
        user_id: "admin-1".into(),
        role: "admin".into(),
    };
    business_claim.provenance =
        ProvenanceRecord::new("admin".into(), "policy".into(), "manual".into());

    // Low-authority Agent claim (conflicting)
    let agent_claim = make_claim_with_confidence("The product price is 200 dollars", tenant, 0.7);

    // Verify tiers differ
    let tier_biz = AuthorityTier::resolve(&business_claim);
    let tier_agent = AuthorityTier::resolve(&agent_claim);
    assert!(tier_biz > tier_agent);

    // Detect conflict — should include authority suggestion
    let conflict = detect_conflict(&business_claim, &agent_claim);
    assert!(conflict.is_some(), "Should detect contradiction");

    let record = conflict.unwrap();
    assert!(
        record.resolution.is_some(),
        "Should have authority suggestion"
    );
    let resolution = record.resolution.unwrap();
    let suggestion = &resolution["authority_suggestion"];
    assert_eq!(suggestion["preferred_tier"].as_str().unwrap(), "canonical",);
}
