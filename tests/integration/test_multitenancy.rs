//! Multi-tenancy isolation tests

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

#[tokio::test]
async fn test_tenant_isolation_on_get() {
    let store = InMemoryClaimStore::new();
    let claim = make_claim("Secret data", "tenant-a");
    let id = claim.id;
    store.insert_claim(&claim).await.unwrap();

    // Same tenant can access
    assert!(store.get_claim(id, "tenant-a").await.is_ok());
    // Different tenant cannot
    assert!(store.get_claim(id, "tenant-b").await.is_err());
}

#[tokio::test]
async fn test_tenant_isolation_on_query() {
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
async fn test_auth_key_per_tenant() {
    let store = InMemoryAuthStore::new();

    let key_a = store
        .create_api_key("tenant-a", vec!["read".into()])
        .await
        .unwrap();
    let key_b = store
        .create_api_key("tenant-b", vec!["write".into()])
        .await
        .unwrap();

    let (tenant_a, perms_a) = store.validate_api_key(&key_a).await.unwrap();
    assert_eq!(tenant_a, "tenant-a");
    assert!(perms_a.contains(&"read".to_string()));

    let (tenant_b, perms_b) = store.validate_api_key(&key_b).await.unwrap();
    assert_eq!(tenant_b, "tenant-b");
    assert!(perms_b.contains(&"write".to_string()));
}

#[tokio::test]
async fn test_invalid_api_key() {
    let store = InMemoryAuthStore::new();
    assert!(store.validate_api_key("nonexistent-key").await.is_err());
}
