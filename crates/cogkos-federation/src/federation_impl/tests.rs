use super::*;
use cogkos_core::{AccessEnvelope, Claimant, ConsolidationStage, NodeType, ProvenanceRecord};

fn create_test_claim(content: &str, confidence: f64) -> cogkos_core::EpistemicClaim {
    let claimant = Claimant::Agent {
        agent_id: "test_agent".to_string(),
        model: "test_model".to_string(),
    };
    let access = AccessEnvelope::new("test_tenant");
    let provenance = ProvenanceRecord::new(
        "test_source".to_string(),
        "test_type".to_string(),
        "test".to_string(),
    );

    let mut claim = cogkos_core::EpistemicClaim::new(
        "test_tenant".to_string(),
        content.to_string(),
        NodeType::Entity,
        claimant,
        access,
        provenance,
    );
    claim.confidence = confidence;
    claim.consolidation_stage = ConsolidationStage::Consolidated;
    claim
}

#[test]
fn test_anonymize_content() {
    let config = AnonymizationConfig::default();
    let anonymizer = InsightAnonymizer::new(config);

    let content = "Apple Inc is located in Cupertino, California";
    let anonymized = anonymizer.anonymize_content(content);

    assert!(!anonymized.contains("Apple Inc"));
    assert!(!anonymized.contains("Cupertino"));
}

#[test]
fn test_anonymize_claim() {
    let config = AnonymizationConfig::default();
    let anonymizer = InsightAnonymizer::new(config);

    let claim = create_test_claim("Test insight content", 0.8);
    let insight = anonymizer.anonymize(&claim, "instance1").unwrap();

    assert!(!insight.anonymized_content.is_empty());
    assert_eq!(insight.confidence, 0.8);
    assert!(!insight.content_hash.is_empty());
    assert!(!insight.source_instance_hash.is_empty());
}

#[test]
fn test_anonymize_below_threshold() {
    let config = AnonymizationConfig {
        min_confidence: 0.7,
        ..Default::default()
    };
    let anonymizer = InsightAnonymizer::new(config);

    let claim = create_test_claim("Test content", 0.5); // Below threshold
    let result = anonymizer.anonymize(&claim, "instance1");

    assert!(result.is_err());
}

#[test]
fn test_time_bucket() {
    use chrono::Datelike;

    let timestamp = chrono::Utc::now();

    let day_bucket = TimeBucket::from_timestamp(timestamp, TimeBucket::Day);
    assert!(day_bucket.contains(&timestamp.year().to_string()));

    let week_bucket = TimeBucket::from_timestamp(timestamp, TimeBucket::Week);
    assert!(week_bucket.starts_with(&timestamp.year().to_string()));
}

#[test]
fn test_cross_instance_auth() {
    let mut auth = CrossInstanceAuthenticator::new();

    let instance_auth = CrossInstanceAuth {
        instance_id: "instance1".to_string(),
        public_key: "key1".to_string(),
        auth_token: "token123".to_string(),
        expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        permissions: vec![FederationPermission::Export, FederationPermission::Import],
    };

    auth.register_instance(instance_auth);

    // Valid auth
    let result = auth.authenticate("token123");
    assert!(result.is_ok());

    // Invalid auth
    let result = auth.authenticate("badtoken");
    assert!(result.is_err());
}

#[test]
fn test_permission_check() {
    let mut auth = CrossInstanceAuthenticator::new();

    let instance_auth = CrossInstanceAuth {
        instance_id: "instance1".to_string(),
        public_key: "key1".to_string(),
        auth_token: "token123".to_string(),
        expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        permissions: vec![FederationPermission::Export],
    };

    auth.register_instance(instance_auth);

    // Has permission
    let result = auth.check_permission("instance1", FederationPermission::Export);
    assert!(result.is_ok());

    // Missing permission
    let result = auth.check_permission("instance1", FederationPermission::Import);
    assert!(result.is_err());
}

#[test]
fn test_export_import_file() {
    let protocol = HttpFederationProtocol::new();

    let insights = vec![AnonymousInsight {
        id: uuid::Uuid::new_v4(),
        content_hash: "hash1".to_string(),
        anonymized_content: "Test insight 1".to_string(),
        node_type: NodeType::Entity,
        confidence: 0.8,
        consolidation_stage: ConsolidationStage::Consolidated,
        domain_tags: vec!["tech".to_string()],
        time_bucket: TimeBucket::Day,
        source_instance_hash: "instance_hash".to_string(),
        statistics: InsightStatistics {
            support_count: 5,
            conflict_count: 1,
            source_diversity: 0.7,
            temporal_range_days: 30,
            normalized_activation: 0.6,
        },
        validation: ValidationMetadata {
            successful_predictions: 10,
            failed_predictions: 2,
            last_validated_bucket: "2024-01-01".to_string(),
            corroboration_count: 3,
        },
    }];

    let path = std::path::Path::new("/tmp/test_export.json");
    protocol.export_to_file_sync(&insights, path).unwrap();

    let imported = protocol.import_from_file_sync(path).unwrap();
    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].content_hash, "hash1");

    // Cleanup
    std::fs::remove_file(path).ok();
}

#[test]
fn test_federation_manager() {
    let manager = FederationManager::new("instance1");

    let claims = vec![
        create_test_claim("Content 1", 0.8),
        create_test_claim("Content 2", 0.9),
    ];

    let insights = manager.export_insights(&claims);
    assert!(!insights.is_empty());
}

#[test]
fn test_validate_imported_insights() {
    let valid_insight = AnonymousInsight {
        id: uuid::Uuid::new_v4(),
        content_hash: "hash1".to_string(),
        anonymized_content: "Valid content".to_string(),
        node_type: NodeType::Entity,
        confidence: 0.8,
        consolidation_stage: ConsolidationStage::Consolidated,
        domain_tags: vec![],
        time_bucket: TimeBucket::Day,
        source_instance_hash: "hash".to_string(),
        statistics: InsightStatistics {
            support_count: 1,
            conflict_count: 0,
            source_diversity: 0.5,
            temporal_range_days: 30,
            normalized_activation: 0.5,
        },
        validation: ValidationMetadata {
            successful_predictions: 0,
            failed_predictions: 0,
            last_validated_bucket: "2024-01-01".to_string(),
            corroboration_count: 0,
        },
    };

    let mut invalid_insight = valid_insight.clone();
    invalid_insight.confidence = 1.5; // Invalid

    let insights = vec![valid_insight, invalid_insight];
    let result = validate_imported_insights(&insights);

    assert_eq!(result.total, 2);
    assert_eq!(result.valid, 1);
    assert_eq!(result.invalid, 1);
    assert!(!result.is_valid());
}
