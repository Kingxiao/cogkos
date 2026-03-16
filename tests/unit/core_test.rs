//! Core library unit tests

#[cfg(test)]
mod models_tests {
    use cogkos_core::models::*;

    #[test]
    fn test_epistemic_claim_new() {
        let claim = EpistemicClaim::new(
            "Test claim content",
            "tenant_1",
            NodeType::Entity,
            Claimant::Human {
                user_id: "user_1".into(),
                role: "researcher".into(),
            },
            AccessEnvelope::new("tenant_1"),
            ProvenanceRecord::new(
                "direct_observation".into(),
                "observation".into(),
                "manual".into(),
            ),
        );

        assert_eq!(claim.tenant_id, "tenant_1");
        assert_eq!(claim.content, "Test claim content");
        assert_eq!(claim.consolidation_stage, ConsolidationStage::FastTrack);
        assert!(claim.is_valid());
    }

    #[test]
    fn test_claim_validity_with_expired_end() {
        let mut claim = EpistemicClaim::new(
            "Test",
            "tenant_1",
            NodeType::Entity,
            Claimant::Human {
                user_id: "u1".into(),
                role: "r".into(),
            },
            AccessEnvelope::new("tenant_1"),
            ProvenanceRecord::new("test".into(), "test".into(), "test".into()),
        );
        claim.t_valid_end = Some(chrono::Utc::now() - chrono::Duration::days(1));
        assert!(!claim.is_valid());
    }

    #[test]
    fn test_claim_validity_retracted() {
        let mut claim = EpistemicClaim::new(
            "Test",
            "tenant_1",
            NodeType::Entity,
            Claimant::System,
            AccessEnvelope::new("tenant_1"),
            ProvenanceRecord::new("test".into(), "test".into(), "test".into()),
        );
        claim.epistemic_status = EpistemicStatus::Retracted;
        assert!(!claim.is_valid());
    }

    #[test]
    fn test_activation_record_access() {
        let mut claim = EpistemicClaim::new(
            "Test",
            "t1",
            NodeType::Entity,
            Claimant::System,
            AccessEnvelope::new("t1"),
            ProvenanceRecord::new("test".into(), "test".into(), "test".into()),
        );
        let w0 = claim.activation_weight;
        claim.record_access(0.1);
        assert!(claim.activation_weight > w0);
        assert_eq!(claim.access_count, 1);
        assert!(claim.last_accessed.is_some());
    }

    #[test]
    fn test_activation_capped_at_one() {
        let mut claim = EpistemicClaim::new(
            "Test",
            "t1",
            NodeType::Entity,
            Claimant::System,
            AccessEnvelope::new("t1"),
            ProvenanceRecord::new("test".into(), "test".into(), "test".into()),
        );
        for _ in 0..100 {
            claim.record_access(0.5);
        }
        assert!(claim.activation_weight <= 1.0);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let claim = EpistemicClaim::new(
            "The sky is blue",
            "tenant-1",
            NodeType::Insight,
            Claimant::Human {
                user_id: "u1".into(),
                role: "observer".into(),
            },
            AccessEnvelope::new("tenant-1"),
            ProvenanceRecord::new("observation".into(), "observation".into(), "manual".into()),
        );
        let json = serde_json::to_string(&claim).unwrap();
        let de: EpistemicClaim = serde_json::from_str(&json).unwrap();
        assert_eq!(claim.id, de.id);
        assert_eq!(claim.content, de.content);
    }

    #[test]
    fn test_node_type_serde() {
        for t in [
            NodeType::Entity,
            NodeType::Relation,
            NodeType::Event,
            NodeType::Attribute,
            NodeType::Prediction,
        ] {
            let json = serde_json::to_string(&t).unwrap();
            let _: NodeType = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn test_knowledge_type_serde() {
        for t in [KnowledgeType::Experiential, KnowledgeType::Business] {
            let json = serde_json::to_string(&t).unwrap();
            let _: KnowledgeType = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn test_consolidation_stage_serde() {
        for s in [
            ConsolidationStage::FastTrack,
            ConsolidationStage::PendingAggregation,
            ConsolidationStage::Consolidated,
            ConsolidationStage::Insight,
            ConsolidationStage::Archived,
        ] {
            let json = serde_json::to_string(&s).unwrap();
            let _: ConsolidationStage = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn test_access_envelope_new() {
        let envelope = AccessEnvelope::new("test-tenant");
        assert_eq!(envelope.visibility, Visibility::Tenant);
        assert!(!envelope.gdpr_applicable);
    }
}

#[cfg(test)]
mod error_tests {
    use cogkos_core::CogKosError;

    #[test]
    fn test_error_codes() {
        assert_eq!(CogKosError::NotFound("x".into()).error_code(), "NOT_FOUND");
        assert_eq!(CogKosError::Forbidden("x".into()).error_code(), "FORBIDDEN");
        assert_eq!(
            CogKosError::InvalidInput("x".into()).error_code(),
            "INVALID_INPUT"
        );
        assert_eq!(
            CogKosError::Database("x".into()).error_code(),
            "DATABASE_ERROR"
        );
    }

    #[test]
    fn test_status_codes() {
        assert_eq!(CogKosError::NotFound("x".into()).status_code(), 404);
        assert_eq!(CogKosError::Forbidden("x".into()).status_code(), 403);
        assert_eq!(CogKosError::InvalidInput("x".into()).status_code(), 400);
        assert_eq!(CogKosError::Database("x".into()).status_code(), 500);
    }
}
