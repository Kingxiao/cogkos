//! P2 Cognitive integration tests
//!
//! Tests that cross crate boundaries and validate CogKOS cognitive behavior:
//! - D5: Multi-agent collective wisdom (four conditions)
//! - D6: Federation node management + anonymization + degradation
//! - D7: Paradigm shift anomaly detection thresholds

use cogkos_core::evolution::paradigm_shift::anomaly::AnomalyDetector;
use cogkos_core::models::*;

fn make_claim_agent(content: &str, agent_id: &str, confidence: f64) -> EpistemicClaim {
    let mut claim = EpistemicClaim::new(
        content,
        "test",
        NodeType::Event,
        Claimant::Agent {
            agent_id: agent_id.into(),
            model: "test-model".into(),
        },
        AccessEnvelope::new("test"),
        ProvenanceRecord::new(
            format!("source-{}", agent_id),
            "agent".into(),
            "test".into(),
        ),
    );
    claim.confidence = confidence;
    claim
}

// ── D5: Multi-Agent Collective Wisdom ────────────────────────

#[test]
fn test_collective_wisdom_four_conditions_healthy() {
    use cogkos_federation::health::{
        calculate_collective_health, InsightSource, Prediction, ProvenanceInfo,
    };

    // 3 independent agents with diverse outputs
    let sources = vec![
        InsightSource {
            source_id: "agent-alpha".into(),
            provenance: ProvenanceInfo {
                source_id: "agent-alpha".into(),
                source_type: "agent".into(),
                upstream_sources: vec![],
            },
            influence: 0.33,
            confidence: 0.8,
            predictions: vec![Prediction {
                content: "Revenue will grow 15%".into(),
                confidence: 0.8,
            }],
        },
        InsightSource {
            source_id: "agent-beta".into(),
            provenance: ProvenanceInfo {
                source_id: "agent-beta".into(),
                source_type: "agent".into(),
                upstream_sources: vec![],
            },
            influence: 0.33,
            confidence: 0.7,
            predictions: vec![Prediction {
                content: "Revenue will grow 12%".into(),
                confidence: 0.7,
            }],
        },
        InsightSource {
            source_id: "agent-gamma".into(),
            provenance: ProvenanceInfo {
                source_id: "agent-gamma".into(),
                source_type: "agent".into(),
                upstream_sources: vec![],
            },
            influence: 0.34,
            confidence: 0.75,
            predictions: vec![Prediction {
                content: "Revenue outlook positive".into(),
                confidence: 0.75,
            }],
        },
    ];

    let health = calculate_collective_health(&sources);

    // With 3 independent, equally-weighted agents:
    assert!(
        health.diversity_score > 0.5,
        "3 diverse agents should score high diversity: {}",
        health.diversity_score
    );
    assert!(
        health.independence_score > 0.5,
        "Independent agents should score high independence: {}",
        health.independence_score
    );
    assert!(
        health.decentralization_score > 0.5,
        "Equal influence should score high decentralization: {}",
        health.decentralization_score
    );
    assert!(
        health.overall_health > 0.5,
        "Overall health should be above warning threshold: {}",
        health.overall_health
    );
}

#[test]
fn test_collective_wisdom_monopoly_warning() {
    use cogkos_federation::health::{
        calculate_collective_health, InsightSource, Prediction, ProvenanceInfo,
    };

    // 1 dominant agent + 2 weak ones
    let sources = vec![
        InsightSource {
            source_id: "dominant".into(),
            provenance: ProvenanceInfo {
                source_id: "dominant".into(),
                source_type: "agent".into(),
                upstream_sources: vec![],
            },
            influence: 0.9,
            confidence: 0.9,
            predictions: vec![Prediction {
                content: "X".into(),
                confidence: 0.9,
            }],
        },
        InsightSource {
            source_id: "weak-1".into(),
            provenance: ProvenanceInfo {
                source_id: "weak-1".into(),
                source_type: "agent".into(),
                upstream_sources: vec![],
            },
            influence: 0.05,
            confidence: 0.5,
            predictions: vec![Prediction {
                content: "Y".into(),
                confidence: 0.5,
            }],
        },
        InsightSource {
            source_id: "weak-2".into(),
            provenance: ProvenanceInfo {
                source_id: "weak-2".into(),
                source_type: "agent".into(),
                upstream_sources: vec![],
            },
            influence: 0.05,
            confidence: 0.5,
            predictions: vec![Prediction {
                content: "Z".into(),
                confidence: 0.5,
            }],
        },
    ];

    let health = calculate_collective_health(&sources);

    // Monopoly should hurt decentralization
    assert!(
        health.decentralization_score < 0.5,
        "Monopoly agent should damage decentralization: {}",
        health.decentralization_score
    );
    assert!(
        !health.warnings.is_empty(),
        "Monopoly should generate warnings"
    );
}

// ── D6: Federation ───────────────────────────────────────────

#[tokio::test]
async fn test_federation_node_lifecycle() {
    let client = cogkos_federation::FederationClientBuilder::new()
        .build()
        .unwrap();

    // Register
    let node = cogkos_federation::FederatedNode::new("node-1", "Test Node", "http://localhost:9999")
        .with_domains(vec!["ai".into(), "finance".into()])
        .with_expertise("ai", 0.9);

    client.register_node(node).await;
    let nodes = client.list_nodes().await;
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].id, "node-1");
    assert_eq!(nodes[0].expertise_for("ai"), 0.9);

    // Unregister
    let removed = client.unregister_node("node-1").await;
    assert!(removed.is_some());
    assert_eq!(client.list_nodes().await.len(), 0);
}

#[test]
fn test_federation_anonymization_removes_entities() {
    use cogkos_federation::{AnonymizationConfig, InsightAnonymizer};

    let config = AnonymizationConfig::default();
    let anonymizer = InsightAnonymizer::new(config);

    let mut claim =
        make_claim_agent("John Smith met with Alice at New York office", "agent-1", 0.9);
    claim.consolidation_stage = ConsolidationStage::Consolidated;
    let insight = anonymizer.anonymize(&claim, "instance-1").unwrap();

    // "First Last" pattern entities should be anonymized
    assert!(
        !insight.anonymized_content.contains("John Smith"),
        "Full name entity should be anonymized: {}",
        insight.anonymized_content
    );
    // Location pattern "at New York" should be anonymized
    assert!(
        !insight.anonymized_content.contains("New York"),
        "Location should be anonymized: {}",
        insight.anonymized_content
    );
}

#[test]
fn test_federation_not_enabled_error() {
    let err = cogkos_federation::FederationError::NotEnabled;
    assert_eq!(format!("{}", err), "Federation not enabled");
}

// ── D7: Paradigm Shift Threshold ─────────────────────────────

#[test]
fn test_paradigm_shift_below_threshold_no_trigger() {
    // Create claims with low prediction errors — should NOT trigger
    let claims: Vec<EpistemicClaim> = (0..20)
        .map(|i| {
            let mut c = make_claim_agent(&format!("Normal claim {}", i), "agent-1", 0.8);
            c.last_prediction_error = Some(0.1); // Low error
            c
        })
        .collect();

    let errors: Vec<f64> = claims.iter().filter_map(|c| c.last_prediction_error).collect();
    let mut detector = AnomalyDetector::new(0.8);
    let result = detector.detect(&claims, &errors);

    assert!(
        result.anomaly_score < 0.8,
        "Low error claims should not trigger paradigm shift: score={}",
        result.anomaly_score
    );
    assert_eq!(
        format!("{:?}", result.recommendation),
        "NoAction",
        "Should recommend no action"
    );
}

#[test]
fn test_paradigm_shift_high_errors_triggers() {
    // Create claims with high prediction errors — should trigger
    let claims: Vec<EpistemicClaim> = (0..20)
        .map(|i| {
            let mut c = make_claim_agent(&format!("Bad prediction claim {}", i), "agent-1", 0.9);
            c.last_prediction_error = Some(0.95); // Very high error
            c.epistemic_status = EpistemicStatus::Contested; // Mark as contested
            c
        })
        .collect();

    let errors: Vec<f64> = claims.iter().filter_map(|c| c.last_prediction_error).collect();
    let mut detector = AnomalyDetector::new(0.8);
    let result = detector.detect(&claims, &errors);

    assert!(
        result.anomaly_score > 0.3,
        "High error claims should elevate anomaly score: {}",
        result.anomaly_score
    );
}

#[test]
fn test_paradigm_shift_empty_claims_safe() {
    let claims: Vec<EpistemicClaim> = vec![];
    let errors: Vec<f64> = vec![];
    let mut detector = AnomalyDetector::new(0.8);
    let result = detector.detect(&claims, &errors);

    assert_eq!(result.anomaly_score, 0.0, "Empty claims should score 0.0");
    assert!(result.anomalies.is_empty(), "No anomalies on empty input");
}
