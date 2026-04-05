//! Graph store integration tests using InMemoryGraphStore
//!
//! Validates: add_node, add_edge, find_related, activation_diffusion,
//! find_path, and tenant_id filtering behavior.

use cogkos_core::models::*;
use cogkos_store::{GraphStore, InMemoryGraphStore};

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

fn make_claim_with_activation(content: &str, tenant: &str, activation: f64) -> EpistemicClaim {
    let mut claim = make_claim(content, tenant);
    claim.activation_weight = activation;
    claim
}

// ── 1. add_node ──────────────────────────────────────────────────

#[tokio::test]
async fn test_add_multiple_nodes() {
    let store = InMemoryGraphStore::new();

    let c1 = make_claim("Rust is fast", "t1");
    let c2 = make_claim("Rust is safe", "t1");
    let c3 = make_claim("Go is concurrent", "t1");

    store.add_node(&c1).await.unwrap();
    store.add_node(&c2).await.unwrap();
    store.add_node(&c3).await.unwrap();

    // Nodes exist — verify via find_related after linking
    store
        .add_edge(c1.id, c2.id, "SIMILAR_TO", 0.9)
        .await
        .unwrap();
    let related = store.find_related(c1.id, "t1", 1, 0.0).await.unwrap();
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].id, c2.id);
}

#[tokio::test]
async fn test_add_node_idempotent() {
    let store = InMemoryGraphStore::new();
    let c1 = make_claim("Node A", "t1");

    store.add_node(&c1).await.unwrap();
    store.add_node(&c1).await.unwrap(); // re-insert same node

    // Should not panic or duplicate
    store.add_edge(c1.id, c1.id, "SELF", 1.0).await.unwrap();
}

// ── 2. add_edge — CAUSES / SIMILAR_TO / DERIVED_FROM ────────────

#[tokio::test]
async fn test_add_edges_multiple_relations() {
    let store = InMemoryGraphStore::new();

    let c1 = make_claim("Event A", "t1");
    let c2 = make_claim("Event B", "t1");
    let c3 = make_claim("Event C", "t1");
    let c4 = make_claim("Event D", "t1");

    for c in [&c1, &c2, &c3, &c4] {
        store.add_node(c).await.unwrap();
    }

    store.add_edge(c1.id, c2.id, "CAUSES", 0.8).await.unwrap();
    store
        .add_edge(c1.id, c3.id, "SIMILAR_TO", 0.6)
        .await
        .unwrap();
    store
        .add_edge(c2.id, c4.id, "DERIVED_FROM", 0.7)
        .await
        .unwrap();

    // c1 at depth=1: BFS visits c2 and c3, then from c2 discovers c4 as neighbor
    // (c4 is added to result but not enqueued further since depth limit reached)
    let related = store.find_related(c1.id, "t1", 1, 0.0).await.unwrap();
    assert_eq!(related.len(), 3);

    let ids: Vec<_> = related.iter().map(|n| n.id).collect();
    assert!(ids.contains(&c2.id));
    assert!(ids.contains(&c3.id));
    assert!(ids.contains(&c4.id));
}

// ── 3. find_related — depth and min_activation ──────────────────

#[tokio::test]
async fn test_find_related_depth_traversal() {
    let store = InMemoryGraphStore::new();

    // Chain: A -> B -> C -> D
    let a = make_claim("A", "t1");
    let b = make_claim("B", "t1");
    let c = make_claim("C", "t1");
    let d = make_claim("D", "t1");

    for n in [&a, &b, &c, &d] {
        store.add_node(n).await.unwrap();
    }

    store.add_edge(a.id, b.id, "CAUSES", 0.8).await.unwrap();
    store.add_edge(b.id, c.id, "CAUSES", 0.7).await.unwrap();
    store.add_edge(c.id, d.id, "CAUSES", 0.6).await.unwrap();

    // depth=1: B is visited, then from B its neighbor C is discovered
    let r1 = store.find_related(a.id, "t1", 1, 0.0).await.unwrap();
    assert_eq!(r1.len(), 2, "depth=1 finds B + C (neighbor of B)");

    // depth=2: B, C visited; from C discover D
    let r2 = store.find_related(a.id, "t1", 2, 0.0).await.unwrap();
    assert_eq!(r2.len(), 3, "depth=2 finds B, C, D");

    // depth=3: same chain fully traversed
    let r3 = store.find_related(a.id, "t1", 3, 0.0).await.unwrap();
    assert_eq!(r3.len(), 3, "depth=3 finds B, C, D (chain exhausted)");
}

#[tokio::test]
async fn test_find_related_min_activation_filter() {
    let store = InMemoryGraphStore::new();

    let a = make_claim_with_activation("A", "t1", 1.0);
    let b_high = make_claim_with_activation("B-high", "t1", 0.9);
    let c_low = make_claim_with_activation("C-low", "t1", 0.1);

    for n in [&a, &b_high, &c_low] {
        store.add_node(n).await.unwrap();
    }

    store
        .add_edge(a.id, b_high.id, "CAUSES", 0.8)
        .await
        .unwrap();
    store.add_edge(a.id, c_low.id, "CAUSES", 0.8).await.unwrap();

    // min_activation=0.5 should only return b_high
    let result = store.find_related(a.id, "t1", 1, 0.5).await.unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, b_high.id);

    // min_activation=0.0 should return both
    let result_all = store.find_related(a.id, "t1", 1, 0.0).await.unwrap();
    assert_eq!(result_all.len(), 2);
}

// ── 4. activation_diffusion — decay behavior ────────────────────

#[tokio::test]
async fn test_activation_diffusion_basic() {
    let store = InMemoryGraphStore::new();

    // A --CAUSES(0.8)--> B --SIMILAR_TO(0.6)--> C
    let a = make_claim("A", "t1");
    let b = make_claim("B", "t1");
    let c = make_claim("C", "t1");

    for n in [&a, &b, &c] {
        store.add_node(n).await.unwrap();
    }

    store.add_edge(a.id, b.id, "CAUSES", 0.8).await.unwrap();
    store.add_edge(b.id, c.id, "SIMILAR_TO", 0.6).await.unwrap();

    let result = store
        .activation_diffusion(a.id, "t1", 1.0, 2, 0.8, 0.01)
        .await
        .unwrap();

    // B should have activation: 1.0 * 0.8 * 0.8 = 0.64
    let b_node = result.iter().find(|n| n.id == b.id);
    assert!(b_node.is_some(), "B should be in diffusion results");
    let b_activation = b_node.unwrap().activation;
    assert!(
        (b_activation - 0.64).abs() < 0.01,
        "B activation should be ~0.64, got {}",
        b_activation
    );

    // C should have activation: 0.64 * 0.6 * 0.8 = 0.3072
    let c_node = result.iter().find(|n| n.id == c.id);
    assert!(c_node.is_some(), "C should be in diffusion results");
    let c_activation = c_node.unwrap().activation;
    assert!(
        (c_activation - 0.3072).abs() < 0.01,
        "C activation should be ~0.3072, got {}",
        c_activation
    );
}

#[tokio::test]
async fn test_activation_diffusion_threshold_cutoff() {
    let store = InMemoryGraphStore::new();

    // Chain: A -> B -> C -> D with decreasing activation
    let a = make_claim("A", "t1");
    let b = make_claim("B", "t1");
    let c = make_claim("C", "t1");
    let d = make_claim("D", "t1");

    for n in [&a, &b, &c, &d] {
        store.add_node(n).await.unwrap();
    }

    store.add_edge(a.id, b.id, "CAUSES", 0.5).await.unwrap();
    store.add_edge(b.id, c.id, "CAUSES", 0.5).await.unwrap();
    store.add_edge(c.id, d.id, "CAUSES", 0.5).await.unwrap();

    // With high threshold, deep nodes should be filtered out
    // B: 1.0 * 0.5 * 0.8 = 0.4
    // C: 0.4 * 0.5 * 0.8 = 0.16
    // D: would be 0.16 * 0.5 * 0.8 = 0.064
    let result = store
        .activation_diffusion(a.id, "t1", 1.0, 3, 0.8, 0.3)
        .await
        .unwrap();

    // Only B should pass threshold 0.3
    assert_eq!(result.len(), 1, "Only B should exceed threshold 0.3");
    assert_eq!(result[0].id, b.id);
}

#[tokio::test]
async fn test_activation_diffusion_excludes_start_node() {
    let store = InMemoryGraphStore::new();

    let a = make_claim("A", "t1");
    let b = make_claim("B", "t1");

    store.add_node(&a).await.unwrap();
    store.add_node(&b).await.unwrap();
    store.add_edge(a.id, b.id, "CAUSES", 0.9).await.unwrap();

    let result = store
        .activation_diffusion(a.id, "t1", 1.0, 1, 0.8, 0.01)
        .await
        .unwrap();

    // Start node (A) should NOT be in results
    assert!(
        result.iter().all(|n| n.id != a.id),
        "Start node should be excluded from diffusion results"
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, b.id);
}

#[tokio::test]
async fn test_activation_diffusion_ignores_non_propagating_relations() {
    let store = InMemoryGraphStore::new();

    let a = make_claim("A", "t1");
    let b = make_claim("B", "t1");
    let c = make_claim("C", "t1");

    for n in [&a, &b, &c] {
        store.add_node(n).await.unwrap();
    }

    // CONTRADICTS is not in the allowed set for propagation
    store
        .add_edge(a.id, b.id, "CONTRADICTS", 0.9)
        .await
        .unwrap();
    store.add_edge(a.id, c.id, "CAUSES", 0.9).await.unwrap();

    let result = store
        .activation_diffusion(a.id, "t1", 1.0, 1, 0.8, 0.01)
        .await
        .unwrap();

    // Only C (via CAUSES) should appear, not B (via CONTRADICTS)
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, c.id);
}

// ── 5. find_path ─────────────────────────────────────────────────

#[tokio::test]
async fn test_find_path_direct() {
    let store = InMemoryGraphStore::new();

    let a = make_claim("A", "t1");
    let b = make_claim("B", "t1");

    store.add_node(&a).await.unwrap();
    store.add_node(&b).await.unwrap();
    store.add_edge(a.id, b.id, "CAUSES", 0.8).await.unwrap();

    let path = store.find_path(a.id, b.id).await.unwrap();
    assert_eq!(path.len(), 2);
    assert_eq!(path[0].id, a.id);
    assert_eq!(path[1].id, b.id);
}

#[tokio::test]
async fn test_find_path_multi_hop() {
    let store = InMemoryGraphStore::new();

    let a = make_claim("A", "t1");
    let b = make_claim("B", "t1");
    let c = make_claim("C", "t1");

    for n in [&a, &b, &c] {
        store.add_node(n).await.unwrap();
    }

    store.add_edge(a.id, b.id, "CAUSES", 0.8).await.unwrap();
    store
        .add_edge(b.id, c.id, "DERIVED_FROM", 0.7)
        .await
        .unwrap();

    let path = store.find_path(a.id, c.id).await.unwrap();
    assert_eq!(path.len(), 3);
    assert_eq!(path[0].id, a.id);
    assert_eq!(path[1].id, b.id);
    assert_eq!(path[2].id, c.id);
}

#[tokio::test]
async fn test_find_path_no_connection() {
    let store = InMemoryGraphStore::new();

    let a = make_claim("A", "t1");
    let b = make_claim("B", "t1");

    store.add_node(&a).await.unwrap();
    store.add_node(&b).await.unwrap();
    // No edge between them

    let path = store.find_path(a.id, b.id).await.unwrap();
    assert!(
        path.is_empty(),
        "No path should exist between disconnected nodes"
    );
}

// ── 6. Tenant isolation (in-memory behavior) ────────────────────
// Note: InMemoryGraphStore does NOT enforce tenant_id filtering
// (the param is ignored). Real isolation is via PostgreSQL RLS.
// These tests document the current in-memory behavior.

#[tokio::test]
async fn test_find_related_tenant_id_param_accepted() {
    let store = InMemoryGraphStore::new();

    let c1 = make_claim("Node A", "tenant-a");
    let c2 = make_claim("Node B", "tenant-b");

    store.add_node(&c1).await.unwrap();
    store.add_node(&c2).await.unwrap();
    store
        .add_edge(c1.id, c2.id, "SIMILAR_TO", 0.9)
        .await
        .unwrap();

    // In-memory store ignores tenant_id — both tenants see the same graph.
    // This is acceptable because production uses FalkorDB with tenant filtering.
    let result_a = store.find_related(c1.id, "tenant-a", 1, 0.0).await.unwrap();
    let result_b = store.find_related(c1.id, "tenant-b", 1, 0.0).await.unwrap();

    // Both return same results (no filtering in memory impl)
    assert_eq!(result_a.len(), result_b.len());
}

#[tokio::test]
async fn test_activation_diffusion_tenant_id_param_accepted() {
    let store = InMemoryGraphStore::new();

    let a = make_claim("A", "tenant-a");
    let b = make_claim("B", "tenant-a");

    store.add_node(&a).await.unwrap();
    store.add_node(&b).await.unwrap();
    store.add_edge(a.id, b.id, "CAUSES", 0.8).await.unwrap();

    // Verify diffusion works with any tenant_id string
    let result = store
        .activation_diffusion(a.id, "tenant-a", 1.0, 1, 0.8, 0.01)
        .await
        .unwrap();
    assert_eq!(result.len(), 1);
}

// ── P1: Activation Diffusion — Relation Weight Differentiation ────

#[tokio::test]
async fn test_diffusion_causes_stronger_than_similar() {
    let store = InMemoryGraphStore::new();

    let a = make_claim("Origin", "t1");
    let b = make_claim("Caused", "t1");
    let c = make_claim("Similar", "t1");

    for n in [&a, &b, &c] {
        store.add_node(n).await.unwrap();
    }

    // CAUSES weight=0.8, SIMILAR weight=0.4 (per get_relation_weight)
    store.add_edge(a.id, b.id, "CAUSES", 0.0).await.unwrap();
    store.add_edge(a.id, c.id, "SIMILAR", 0.0).await.unwrap();

    let result = store
        .activation_diffusion(a.id, "t1", 1.0, 1, 0.8, 0.01)
        .await
        .unwrap();

    let b_activation = result.iter().find(|n| n.id == b.id).map(|n| n.activation);
    let c_activation = result.iter().find(|n| n.id == c.id).map(|n| n.activation);

    assert!(
        b_activation.is_some(),
        "CAUSES path should propagate"
    );
    // SIMILAR(0.4) * 0.8(decay) = 0.32, which is above threshold 0.01
    // but CAUSES(0.8) * 0.8(decay) = 0.64 should be higher
    if let (Some(b_act), Some(c_act)) = (b_activation, c_activation) {
        assert!(
            b_act > c_act,
            "CAUSES ({}) should propagate more activation than SIMILAR ({})",
            b_act, c_act
        );
    }
}

#[tokio::test]
async fn test_diffusion_family_relations_strongest() {
    let store = InMemoryGraphStore::new();

    let parent = make_claim("Parent", "t1");
    let child_family = make_claim("Family", "t1");
    let child_activity = make_claim("Activity", "t1");

    for n in [&parent, &child_family, &child_activity] {
        store.add_node(n).await.unwrap();
    }

    // HAS_FAMILY=0.8, DOES_ACTIVITY=0.5
    store
        .add_edge(parent.id, child_family.id, "HAS_FAMILY", 0.0)
        .await
        .unwrap();
    store
        .add_edge(parent.id, child_activity.id, "DOES_ACTIVITY", 0.0)
        .await
        .unwrap();

    let result = store
        .activation_diffusion(parent.id, "t1", 1.0, 1, 0.8, 0.01)
        .await
        .unwrap();

    let family_act = result
        .iter()
        .find(|n| n.id == child_family.id)
        .map(|n| n.activation)
        .unwrap_or(0.0);
    let activity_act = result
        .iter()
        .find(|n| n.id == child_activity.id)
        .map(|n| n.activation)
        .unwrap_or(0.0);

    assert!(
        family_act > activity_act,
        "HAS_FAMILY ({}) should propagate more than DOES_ACTIVITY ({})",
        family_act, activity_act
    );
}

#[tokio::test]
async fn test_diffusion_cycle_no_infinite_loop() {
    let store = InMemoryGraphStore::new();

    let a = make_claim("A", "t1");
    let b = make_claim("B", "t1");
    let c = make_claim("C", "t1");

    for n in [&a, &b, &c] {
        store.add_node(n).await.unwrap();
    }

    // Create cycle: A→B→C→A
    store.add_edge(a.id, b.id, "CAUSES", 0.8).await.unwrap();
    store.add_edge(b.id, c.id, "CAUSES", 0.8).await.unwrap();
    store.add_edge(c.id, a.id, "CAUSES", 0.8).await.unwrap();

    // Should complete without hanging
    let result = store
        .activation_diffusion(a.id, "t1", 1.0, 5, 0.8, 0.01)
        .await
        .unwrap();

    // B and C should be found (A is excluded as start node)
    assert_eq!(result.len(), 2, "Cycle should still find B and C without infinite loop");
}

// ── 7. FalkorDB real-connection tests (requires running FalkorDB) ─
// Run with: cargo test --test test_graph falkor -- --ignored
// CI runs these in the integration job with FalkorDB service container.

#[tokio::test]
#[ignore = "requires running FalkorDB instance (FALKORDB_URL env var)"]
async fn test_falkordb_add_node_and_find_related() {
    let url = std::env::var("FALKORDB_URL").unwrap_or_else(|_| "redis://localhost:6381".into());
    let graph = std::env::var("FALKORDB_GRAPH").unwrap_or_else(|_| "cogkos_test".into());

    let cfg = deadpool_redis::Config::from_url(&url);
    let pool = cfg
        .create_pool(Some(deadpool_redis::Runtime::Tokio1))
        .expect("Failed to create Redis pool");

    // Verify connectivity
    let mut conn = pool.get().await.expect("Failed to connect to FalkorDB");
    let _: String = deadpool_redis::redis::cmd("PING")
        .query_async(&mut conn)
        .await
        .expect("FalkorDB PING failed");
    drop(conn);

    let store = cogkos_store::FalkorStore::new(pool, &graph);

    let a = make_claim("FalkorDB node A", "t-falkor");
    let b = make_claim("FalkorDB node B", "t-falkor");

    store.add_node(&a).await.unwrap();
    store.add_node(&b).await.unwrap();
    store
        .add_edge(a.id, b.id, "SIMILAR_TO", 0.85)
        .await
        .unwrap();

    let related = store.find_related(a.id, "t-falkor", 1, 0.0).await.unwrap();
    assert!(
        !related.is_empty(),
        "FalkorDB find_related should return results"
    );
    assert!(
        related.iter().any(|n| n.id == b.id),
        "Related nodes should include B"
    );
}

#[tokio::test]
#[ignore = "requires running FalkorDB instance (FALKORDB_URL env var)"]
async fn test_falkordb_activation_diffusion() {
    let url = std::env::var("FALKORDB_URL").unwrap_or_else(|_| "redis://localhost:6381".into());
    let graph = std::env::var("FALKORDB_GRAPH").unwrap_or_else(|_| "cogkos_test_diffusion".into());

    let cfg = deadpool_redis::Config::from_url(&url);
    let pool = cfg
        .create_pool(Some(deadpool_redis::Runtime::Tokio1))
        .expect("Failed to create Redis pool");

    let store = cogkos_store::FalkorStore::new(pool, &graph);

    let a = make_claim("Diffusion A", "t-diff");
    let b = make_claim("Diffusion B", "t-diff");
    let c = make_claim("Diffusion C", "t-diff");

    for n in [&a, &b, &c] {
        store.add_node(n).await.unwrap();
    }

    store.add_edge(a.id, b.id, "CAUSES", 0.8).await.unwrap();
    store.add_edge(b.id, c.id, "SIMILAR_TO", 0.6).await.unwrap();

    let result = store
        .activation_diffusion(a.id, "t-diff", 1.0, 2, 0.8, 0.01)
        .await
        .unwrap();

    // B and C should appear in diffusion results
    assert!(
        result.iter().any(|n| n.id == b.id),
        "Diffusion should reach B"
    );
}
