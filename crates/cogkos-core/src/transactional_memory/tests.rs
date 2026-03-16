use super::*;
use chrono::Utc;
use std::collections::HashMap;

fn create_test_entry(instance_id: &str) -> CatalogEntry {
    let mut expertise = HashMap::new();
    expertise.insert("tech".to_string(), 0.9);
    expertise.insert("science".to_string(), 0.7);

    CatalogEntry {
        instance_id: instance_id.to_string(),
        domain_tags: vec!["tech".to_string(), "science".to_string()],
        expertise_scores: expertise,
        knowledge_count: 1000,
        last_updated: Utc::now(),
        api_endpoint: format!("https://{}.example.com/api", instance_id),
        auth_method: AuthMethod::None,
        capabilities: vec![
            InstanceCapability::KnowledgeRead,
            InstanceCapability::QueryProcessing,
        ],
    }
}

#[test]
fn test_catalog_crud() {
    let mut catalog = MetaKnowledgeCatalog::new();

    // Create
    let entry = create_test_entry("instance1");
    catalog.upsert(entry.clone()).unwrap();

    // Read
    let retrieved = catalog.get("instance1").unwrap();
    assert_eq!(retrieved.instance_id, "instance1");

    // Read by domain
    let tech_instances = catalog.get_by_domain("tech");
    assert_eq!(tech_instances.len(), 1);

    // Update
    let update = CatalogUpdate {
        knowledge_count: Some(2000),
        ..Default::default()
    };
    catalog.update("instance1", update).unwrap();

    let updated = catalog.get("instance1").unwrap();
    assert_eq!(updated.knowledge_count, 2000);

    // Delete
    catalog.delete("instance1").unwrap();
    assert!(catalog.get("instance1").is_none());
}

#[test]
fn test_cross_instance_router() {
    let mut router = CrossInstanceRouter::new();

    let entry1 = create_test_entry("instance1");
    let mut entry2 = create_test_entry("instance2");
    entry2.domain_tags = vec!["business".to_string()];
    entry2.expertise_scores = {
        let mut map = HashMap::new();
        map.insert("business".to_string(), 0.85);
        map
    };

    router.register_instance(entry1).unwrap();
    router.register_instance(entry2).unwrap();

    // Route query for tech domain
    let decision = router.route_query("test query", &["tech".to_string()]);
    assert!(decision.target_instances.contains(&"instance1".to_string()));

    // Route query for business domain
    let decision = router.route_query("test query", &["business".to_string()]);
    assert!(decision.target_instances.contains(&"instance2".to_string()));
}

#[test]
fn test_distributed_transaction() {
    use crate::models::MetaKnowledgeEntry;

    let tx = DistributedTransaction::new("coordinator1")
        .add_participant("instance1")
        .add_participant("instance2")
        .add_operation(TransactionOperation::SyncMetadata {
            entry: MetaKnowledgeEntry {
                instance_id: "instance1".to_string(),
                domain_tags: vec!["tech".to_string()],
                expertise_score: 0.9,
                last_updated: Utc::now(),
            },
        });

    assert_eq!(tx.participants.len(), 2);
    assert_eq!(tx.operations.len(), 1);
    assert_eq!(tx.coordinator, "coordinator1");
}

#[test]
fn test_transaction_coordinator() {
    let mut coordinator = TransactionCoordinator::new();

    let tx = coordinator.begin_transaction("coordinator1");
    let tx_id = tx.id;

    let retrieved = coordinator.get_transaction(tx_id).unwrap();
    assert_eq!(retrieved.id, tx_id);

    let active = coordinator.active_transactions();
    assert_eq!(active.len(), 1);
}

#[test]
fn test_transactional_memory() {
    let mut tm = TransactionalMemory::new("local_instance");

    let entry = create_test_entry("remote1");
    tm.register_remote_instance(entry).unwrap();

    let decision = tm.route_federated_query("test", &["tech".to_string()]);
    assert!(!decision.target_instances.is_empty());
}

#[test]
fn test_domain_statistics() {
    let mut catalog = MetaKnowledgeCatalog::new();

    let entry1 = create_test_entry("instance1");
    let mut entry2 = create_test_entry("instance2");
    entry2.domain_tags = vec!["tech".to_string()]; // Also tech

    catalog.upsert(entry1).unwrap();
    catalog.upsert(entry2).unwrap();

    let stats = catalog.domain_statistics();
    assert_eq!(stats.get("tech"), Some(&2));
    assert_eq!(stats.get("science"), Some(&1));
}
