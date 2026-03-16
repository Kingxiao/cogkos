//! KnowledgeType + EntityRef tests

use cogkos_core::models::*;

#[test]
fn test_knowledge_type_default_is_experiential() {
    assert_eq!(KnowledgeType::default(), KnowledgeType::Experiential);
}

#[test]
fn test_knowledge_type_variants() {
    let _ = KnowledgeType::Experiential;
    let _ = KnowledgeType::Business;
}

#[test]
fn test_knowledge_type_display() {
    assert_eq!(format!("{}", KnowledgeType::Experiential), "Experiential");
    assert_eq!(format!("{}", KnowledgeType::Business), "Business");
}

#[test]
fn test_epistemic_claim_has_knowledge_type() {
    let claim = EpistemicClaim::new(
        "test content",
        "test-tenant",
        NodeType::Entity,
        Claimant::System,
        AccessEnvelope::new("test-tenant"),
        ProvenanceRecord::new("test".into(), "test".into(), "test".into()),
    );
    assert_eq!(claim.knowledge_type, KnowledgeType::Experiential);
    assert_eq!(claim.version, 1);
    assert!(claim.superseded_by.is_none());
    assert!(claim.entity_refs.is_empty());
}

#[test]
fn test_entity_ref_serialization() {
    let er = EntityRef { entity_type: "customer".into(), entity_id: "cust-001".into() };
    let json = serde_json::to_string(&er).unwrap();
    assert!(json.contains("customer"));
    assert!(json.contains("cust-001"));
}

#[test]
fn test_entity_ref_deserialization() {
    let json = r#"{"entity_type":"customer","entity_id":"cust-001"}"#;
    let er: EntityRef = serde_json::from_str(json).unwrap();
    assert_eq!(er.entity_type, "customer");
    assert_eq!(er.entity_id, "cust-001");
}

#[test]
fn test_business_knowledge_version_increment() {
    let mut claim = EpistemicClaim::new(
        "test content", "test-tenant", NodeType::Entity,
        Claimant::System,
        AccessEnvelope::new("test-tenant"), ProvenanceRecord::new("test".into(), "test".into(), "test".into()),
    );
    claim.knowledge_type = KnowledgeType::Business;
    assert_eq!(claim.version, 1);
    claim.version += 1;
    assert_eq!(claim.version, 2);
}

#[test]
fn test_knowledge_type_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<KnowledgeType>();
    assert_send_sync::<EntityRef>();
}

#[test]
fn test_epistemic_claim_struct_has_all_fields() {
    let claim = EpistemicClaim::new(
        "test content", "test-tenant", NodeType::Insight,
        Claimant::Human { user_id: "user-1".into(), role: "analyst".into() },
        AccessEnvelope::new("test-tenant"), ProvenanceRecord::new("test".into(), "test".into(), "test".into()),
    );
    assert!(!claim.id.is_nil());
    assert_eq!(claim.tenant_id, "test-tenant");
    assert_eq!(claim.content, "test content");
    assert_eq!(claim.knowledge_type, KnowledgeType::Experiential);
    assert_eq!(claim.confidence, 0.5);
    assert_eq!(claim.consolidation_stage, ConsolidationStage::FastTrack);
    assert_eq!(claim.version, 1);
    assert_eq!(claim.durability, 1.0);
    assert_eq!(claim.activation_weight, 0.5);
    assert_eq!(claim.access_count, 0);
}
