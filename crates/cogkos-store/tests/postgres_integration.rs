//! Integration tests for PostgresStore against a real PostgreSQL database.
//!
//! Prerequisites: docker compose -f docker-compose.test.yml up -d
//! Run: TEST_DATABASE_URL=postgres://cogkos:cogkos_test@localhost:5433/cogkos_test cargo test -p cogkos-store --test postgres_integration -- --ignored

use cogkos_core::models::*;
use cogkos_store::postgres::PostgresStore;
use cogkos_store::{ClaimStore, GapStore, KnowledgeGapRecord};
use sqlx::PgPool;
use uuid::Uuid;

async fn setup() -> Option<(PostgresStore, PgPool)> {
    let url = match std::env::var("TEST_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => return None,
    };

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&url)
        .await
        .expect("Failed to connect to test database");

    // Run migrations
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let store = PostgresStore::new(pool.clone());
    Some((store, pool))
}

fn make_claim(tenant_id: &str, content: &str) -> EpistemicClaim {
    let now = chrono::Utc::now();
    EpistemicClaim {
        id: Uuid::new_v4(),
        tenant_id: tenant_id.to_string(),
        content: content.to_string(),
        node_type: NodeType::Entity,
        knowledge_type: KnowledgeType::Experiential,
        structured_content: None,
        epistemic_status: EpistemicStatus::Asserted,
        confidence: 0.8,
        consolidation_stage: ConsolidationStage::FastTrack,
        claimant: Claimant::System,
        provenance: ProvenanceRecord::new("test".into(), "test".into(), "test".into()),
        access_envelope: AccessEnvelope::new("test-tenant"),
        activation_weight: 0.5,
        access_count: 0,
        last_accessed: None,
        t_valid_start: now,
        t_valid_end: None,
        t_known: now,
        vector_id: None,
        last_prediction_error: None,
        derived_from: vec![],
        superseded_by: None,
        entity_refs: vec![],
        needs_revalidation: false,
        version: 1,
        durability: 1.0,
        created_at: now,
        updated_at: now,
        metadata: serde_json::Map::new(),
    }
}

/// Clean up test data after each test
async fn cleanup(pool: &PgPool, tenant_id: &str) {
    // RLS bypassed by superuser; delete directly
    let _ = sqlx::query("DELETE FROM conflict_records WHERE tenant_id = $1")
        .bind(tenant_id)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM epistemic_claims WHERE tenant_id = $1")
        .bind(tenant_id)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM knowledge_gaps WHERE tenant_id = $1")
        .bind(tenant_id)
        .execute(pool)
        .await;
}

#[tokio::test]
#[ignore]
async fn test_insert_and_get_claim() {
    let Some((store, pool)) = setup().await else {
        return;
    };
    let tenant = "test-insert";
    cleanup(&pool, tenant).await;

    let claim = make_claim(tenant, "The earth orbits the sun");
    let id = store.insert_claim(&claim).await.unwrap();
    assert_eq!(id, claim.id);

    let fetched = store.get_claim(id, tenant).await.unwrap();
    assert_eq!(fetched.id, claim.id);
    assert_eq!(fetched.content, "The earth orbits the sun");
    assert_eq!(fetched.tenant_id, tenant);
    assert!(fetched.confidence > 0.79 && fetched.confidence < 0.81);

    cleanup(&pool, tenant).await;
}

#[tokio::test]
#[ignore]
async fn test_update_claim() {
    let Some((store, pool)) = setup().await else {
        return;
    };
    let tenant = "test-update";
    cleanup(&pool, tenant).await;

    let mut claim = make_claim(tenant, "Initial content");
    store.insert_claim(&claim).await.unwrap();

    claim.content = "Updated content".to_string();
    claim.confidence = 0.95;
    store.update_claim(&claim).await.unwrap();

    let fetched = store.get_claim(claim.id, tenant).await.unwrap();
    assert_eq!(fetched.content, "Updated content");
    assert!(fetched.confidence > 0.94);

    cleanup(&pool, tenant).await;
}

#[tokio::test]
#[ignore]
async fn test_delete_claim() {
    let Some((store, pool)) = setup().await else {
        return;
    };
    let tenant = "test-delete";
    cleanup(&pool, tenant).await;

    let claim = make_claim(tenant, "To be deleted");
    store.insert_claim(&claim).await.unwrap();
    store.delete_claim(claim.id, tenant).await.unwrap();

    let result = store.get_claim(claim.id, tenant).await;
    assert!(result.is_err());

    cleanup(&pool, tenant).await;
}

#[tokio::test]
#[ignore]
async fn test_query_claims() {
    let Some((store, pool)) = setup().await else {
        return;
    };
    let tenant = "test-query";
    cleanup(&pool, tenant).await;

    for i in 0..5 {
        let claim = make_claim(tenant, &format!("Claim number {}", i));
        store.insert_claim(&claim).await.unwrap();
    }

    let results = store.query_claims(tenant, &[]).await.unwrap();
    assert_eq!(results.len(), 5);

    cleanup(&pool, tenant).await;
}

#[tokio::test]
#[ignore]
async fn test_tenant_isolation() {
    let Some((store, pool)) = setup().await else {
        return;
    };
    let tenant_a = "test-iso-a";
    let tenant_b = "test-iso-b";
    cleanup(&pool, tenant_a).await;
    cleanup(&pool, tenant_b).await;

    store
        .insert_claim(&make_claim(tenant_a, "Tenant A data"))
        .await
        .unwrap();
    store
        .insert_claim(&make_claim(tenant_b, "Tenant B data"))
        .await
        .unwrap();

    let a_claims = store.query_claims(tenant_a, &[]).await.unwrap();
    let b_claims = store.query_claims(tenant_b, &[]).await.unwrap();

    assert_eq!(a_claims.len(), 1);
    assert_eq!(b_claims.len(), 1);
    assert_eq!(a_claims[0].content, "Tenant A data");
    assert_eq!(b_claims[0].content, "Tenant B data");

    // tenant_a cannot see tenant_b's data
    let cross = store.get_claim(b_claims[0].id, tenant_a).await;
    assert!(cross.is_err());

    cleanup(&pool, tenant_a).await;
    cleanup(&pool, tenant_b).await;
}

#[tokio::test]
#[ignore]
async fn test_insert_conflict() {
    let Some((store, pool)) = setup().await else {
        return;
    };
    let tenant = "test-conflict";
    cleanup(&pool, tenant).await;

    let claim_a = make_claim(tenant, "The sky is blue");
    let claim_b = make_claim(tenant, "The sky is green");
    store.insert_claim(&claim_a).await.unwrap();
    store.insert_claim(&claim_b).await.unwrap();

    let conflict = ConflictRecord {
        id: Uuid::new_v4(),
        tenant_id: tenant.to_string(),
        claim_a_id: claim_a.id,
        claim_b_id: claim_b.id,
        conflict_type: ConflictType::DirectContradiction,
        severity: 0.9,
        description: Some("Color disagreement".to_string()),
        detected_at: chrono::Utc::now(),
        resolved_at: None,
        resolution: None,
        resolution_status: ResolutionStatus::Open,
        resolution_note: None,
        elevated_insight_id: None,
    };

    store.insert_conflict(&conflict).await.unwrap();

    let conflicts = store
        .get_conflicts_for_claim(claim_a.id, tenant)
        .await
        .unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].severity, 0.9);

    cleanup(&pool, tenant).await;
}

#[tokio::test]
#[ignore]
async fn test_record_gap() {
    let Some((store, pool)) = setup().await else {
        return;
    };
    let tenant = "test-gap";
    cleanup(&pool, tenant).await;

    let gap = KnowledgeGapRecord {
        gap_id: Uuid::new_v4(),
        tenant_id: tenant.to_string(),
        domain: "physics".to_string(),
        description: "Missing quantum mechanics knowledge".to_string(),
        priority: "high".to_string(),
        status: "open".to_string(),
        reported_at: chrono::Utc::now(),
        filled_at: None,
    };

    let id = store.record_gap(&gap).await.unwrap();
    assert_eq!(id, gap.gap_id);

    cleanup(&pool, tenant).await;
}

#[tokio::test]
#[ignore]
async fn test_update_activation() {
    let Some((store, pool)) = setup().await else {
        return;
    };
    let tenant = "test-activation";
    cleanup(&pool, tenant).await;

    let claim = make_claim(tenant, "Activation test");
    store.insert_claim(&claim).await.unwrap();

    store
        .update_activation(claim.id, tenant, 0.3)
        .await
        .unwrap();

    let fetched = store.get_claim(claim.id, tenant).await.unwrap();
    assert!(
        fetched.activation_weight > 0.79,
        "Expected ~0.8 (0.5+0.3), got {}",
        fetched.activation_weight
    );

    cleanup(&pool, tenant).await;
}

#[tokio::test]
#[ignore]
async fn test_resolve_conflict() {
    let Some((store, pool)) = setup().await else {
        return;
    };
    let tenant = "test-resolve";
    cleanup(&pool, tenant).await;

    let claim_a = make_claim(tenant, "Product is great");
    let claim_b = make_claim(tenant, "Product is terrible");
    store.insert_claim(&claim_a).await.unwrap();
    store.insert_claim(&claim_b).await.unwrap();

    let conflict = ConflictRecord {
        id: Uuid::new_v4(),
        tenant_id: tenant.to_string(),
        claim_a_id: claim_a.id,
        claim_b_id: claim_b.id,
        conflict_type: ConflictType::DirectContradiction,
        severity: 0.85,
        description: Some("Quality disagreement".to_string()),
        detected_at: chrono::Utc::now(),
        resolved_at: None,
        resolution: None,
        resolution_status: ResolutionStatus::Open,
        resolution_note: None,
        elevated_insight_id: None,
    };
    let conflict_id = conflict.id;
    store.insert_conflict(&conflict).await.unwrap();

    // Resolve it
    store
        .resolve_conflict(
            conflict_id,
            tenant,
            ResolutionStatus::Accepted,
            Some("Context dependent".into()),
        )
        .await
        .unwrap();

    // Verify resolved
    let conflicts = store
        .get_conflicts_for_claim(claim_a.id, tenant)
        .await
        .unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].resolution_status, ResolutionStatus::Accepted);
    assert!(conflicts[0].resolved_at.is_some());

    cleanup(&pool, tenant).await;
}

#[tokio::test]
#[ignore]
async fn test_list_tenants() {
    let Some((store, pool)) = setup().await else {
        return;
    };
    let tenant_x = "test-tenant-x";
    let tenant_y = "test-tenant-y";
    cleanup(&pool, tenant_x).await;
    cleanup(&pool, tenant_y).await;

    store
        .insert_claim(&make_claim(tenant_x, "Claim X"))
        .await
        .unwrap();
    store
        .insert_claim(&make_claim(tenant_y, "Claim Y"))
        .await
        .unwrap();

    let tenants = store.list_tenants().await.unwrap();
    assert!(tenants.contains(&tenant_x.to_string()));
    assert!(tenants.contains(&tenant_y.to_string()));

    cleanup(&pool, tenant_x).await;
    cleanup(&pool, tenant_y).await;
}

#[tokio::test]
#[ignore]
async fn test_e2e_ingest_conflict_resolve() {
    let Some((store, pool)) = setup().await else {
        return;
    };
    let tenant = "test-e2e";
    cleanup(&pool, tenant).await;

    // Step 1: Ingest multiple claims
    let c1 = make_claim(tenant, "Revenue grew 20% in 2025");
    let c2 = make_claim(tenant, "Revenue declined 5% in 2025");
    let c3 = make_claim(tenant, "The team expanded to 50 people");
    store.insert_claim(&c1).await.unwrap();
    store.insert_claim(&c2).await.unwrap();
    store.insert_claim(&c3).await.unwrap();

    // Step 2: Detect conflict between c1 and c2
    let conflict = ConflictRecord {
        id: Uuid::new_v4(),
        tenant_id: tenant.to_string(),
        claim_a_id: c1.id,
        claim_b_id: c2.id,
        conflict_type: ConflictType::DirectContradiction,
        severity: 0.9,
        description: Some("Revenue growth contradiction".to_string()),
        detected_at: chrono::Utc::now(),
        resolved_at: None,
        resolution: None,
        resolution_status: ResolutionStatus::Open,
        resolution_note: None,
        elevated_insight_id: None,
    };
    store.insert_conflict(&conflict).await.unwrap();

    // Step 3: Query — verify claims and conflicts exist
    let claims = store.query_claims(tenant, &[]).await.unwrap();
    assert_eq!(claims.len(), 3);

    let conflicts = store.get_conflicts_for_claim(c1.id, tenant).await.unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].resolution_status, ResolutionStatus::Open);

    // Step 4: Resolve conflict
    store
        .resolve_conflict(
            conflict.id,
            tenant,
            ResolutionStatus::Dismissed,
            Some("c2 source unreliable".into()),
        )
        .await
        .unwrap();

    // Step 5: Update c2 confidence down (superseded)
    store.update_confidence(c2.id, tenant, 0.1).await.unwrap();
    let updated = store.get_claim(c2.id, tenant).await.unwrap();
    assert!(updated.confidence < 0.2);

    // Step 6: Verify conflict resolved
    let conflicts = store.get_conflicts_for_claim(c1.id, tenant).await.unwrap();
    assert_eq!(conflicts[0].resolution_status, ResolutionStatus::Dismissed);

    cleanup(&pool, tenant).await;
}
