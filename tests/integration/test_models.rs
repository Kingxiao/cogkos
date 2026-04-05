//! Model serialization and validation tests

use cogkos_core::models::*;

#[test]
fn test_conflict_record_creation() {
    let record = ConflictRecord {
        id: uuid::Uuid::new_v4(),
        tenant_id: "t1".into(),
        claim_a_id: uuid::Uuid::new_v4(),
        claim_b_id: uuid::Uuid::new_v4(),
        conflict_type: ConflictType::DirectContradiction,
        severity: 0.8,
        description: Some("A contradicts B".into()),
        detected_at: chrono::Utc::now(),
        resolved_at: None,
        resolution: None,
        resolution_status: ResolutionStatus::Open,
        resolution_note: None,
        elevated_insight_id: None,
    };
    assert_eq!(record.severity, 0.8);
    assert_eq!(record.resolution_status, ResolutionStatus::Open);
}

#[test]
fn test_conflict_types() {
    let types = vec![
        ConflictType::DirectContradiction,
        ConflictType::ContextDependent,
        ConflictType::TemporalShift,
        ConflictType::TemporalInconsistency,
        ConflictType::SourceDisagreement,
        ConflictType::ConfidenceMismatch,
        ConflictType::ContextualDifference,
    ];
    for t in types {
        let json = serde_json::to_string(&t).unwrap();
        let _: ConflictType = serde_json::from_str(&json).unwrap();
    }
}

#[test]
fn test_visibility_levels() {
    let levels = vec![
        Visibility::Public,
        Visibility::CrossTenant,
        Visibility::Tenant,
        Visibility::Team,
        Visibility::Private,
    ];
    for v in levels {
        let json = serde_json::to_string(&v).unwrap();
        let _: Visibility = serde_json::from_str(&json).unwrap();
    }
}

#[test]
fn test_provenance_record() {
    let p = ProvenanceRecord::new("manual_entry".into(), "manual".into(), "direct".into());
    assert_eq!(p.source_id, "manual_entry");
    assert_eq!(p.source_type, "manual");
    let json = serde_json::to_string(&p).unwrap();
    let _: ProvenanceRecord = serde_json::from_str(&json).unwrap();
}

#[test]
fn test_resolution_status_serde() {
    for s in [
        ResolutionStatus::Open,
        ResolutionStatus::Elevated,
        ResolutionStatus::Dismissed,
        ResolutionStatus::Accepted,
    ] {
        let json = serde_json::to_string(&s).unwrap();
        let _: ResolutionStatus = serde_json::from_str(&json).unwrap();
    }
}
