//! Unit tests for evolution engine

#[cfg(test)]
mod tests {
    use cogkos_core::evolution::bayesian::*;
    use cogkos_core::evolution::conflict::detect_conflict;
    use cogkos_core::models::*;

    fn make_claim(content: &str, confidence: f64) -> EpistemicClaim {
        let mut claim = EpistemicClaim::new(
            content,
            "test",
            NodeType::Entity,
            Claimant::Human {
                user_id: "user1".into(),
                role: "tester".into(),
            },
            AccessEnvelope::new("test"),
            ProvenanceRecord::new("test_source".into(), "test".into(), "test".into()),
        );
        claim.confidence = confidence;
        claim
    }

    #[test]
    fn test_log_odds_at_0_5() {
        let result = log_odds(0.5);
        assert!((result - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_log_odds_at_0_9() {
        assert!(log_odds(0.9) > 0.0);
    }

    #[test]
    fn test_log_odds_at_0_1() {
        assert!(log_odds(0.1) < 0.0);
    }

    #[test]
    fn test_odds_to_prob_inverse() {
        let p = 0.75;
        let lo = log_odds(p);
        let p_back = odds_to_prob(lo);
        assert!((p - p_back).abs() < 0.001);
    }

    #[test]
    fn test_bayesian_aggregate_empty() {
        let claims: Vec<EpistemicClaim> = vec![];
        let result = bayesian_aggregate(&claims);
        assert!((result - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_bayesian_aggregate_single_high() {
        let claim = make_claim("test", 0.8);
        let result = bayesian_aggregate(&[claim]);
        assert!(result > 0.7);
    }

    #[test]
    fn test_bayesian_aggregate_multiple_agreeing() {
        let c1 = make_claim("test", 0.8);
        let c2 = make_claim("test", 0.7);
        let c3 = make_claim("test", 0.75);
        let result = bayesian_aggregate(&[c1, c2, c3]);
        assert!(result > 0.8);
    }

    #[test]
    fn test_conflict_detection_same_id() {
        let claim = make_claim("content", 0.9);
        let result = detect_conflict(&claim, &claim);
        assert!(
            result.is_none(),
            "Same claim should not conflict with itself"
        );
    }

    #[test]
    fn test_conflict_detection_different_content() {
        let c1 = make_claim("Earth is round", 0.9);
        let c2 = make_claim("Completely different topic about quantum physics", 0.9);
        let result = detect_conflict(&c1, &c2);
        assert!(result.is_none(), "Different content should not conflict");
    }

    // ── D2: Conflict Detection — 7 Types ────────────────────────

    fn make_claim_with_source(content: &str, confidence: f64, source_id: &str) -> EpistemicClaim {
        let mut claim = EpistemicClaim::new(
            content,
            "test",
            NodeType::Event,
            Claimant::Agent {
                agent_id: "test-agent".into(),
                model: "test".into(),
            },
            AccessEnvelope::new("test"),
            ProvenanceRecord::new(source_id.into(), "agent".into(), "test".into()),
        );
        claim.confidence = confidence;
        claim
    }

    #[test]
    fn test_conflict_d2_direct_contradiction() {
        let c1 = make_claim("the project is not delayed", 0.9);
        let c2 = make_claim("the project is delayed significantly", 0.9);
        let result = detect_conflict(&c1, &c2);
        assert!(
            result.is_some(),
            "Negation should trigger DirectContradiction"
        );
        assert_eq!(
            result.unwrap().conflict_type,
            ConflictType::DirectContradiction
        );
    }

    #[test]
    fn test_conflict_d2_confidence_mismatch_numeric() {
        let c1 = make_claim_with_source("revenue grew 15 percent last quarter", 0.9, "finance");
        let c2 = make_claim_with_source("revenue grew 3 percent last quarter", 0.9, "sales");
        let result = detect_conflict(&c1, &c2);
        assert!(
            result.is_some(),
            "Numbers 15 vs 3 should trigger conflict (>50% diff)"
        );
    }

    #[test]
    fn test_conflict_d2_confidence_gap() {
        let c1 = make_claim_with_source(
            "the project deadline is march fifteenth confirmed",
            0.95,
            "manager",
        );
        let c2 = make_claim_with_source(
            "the project deadline is around march or maybe april",
            0.4,
            "intern",
        );
        let result = detect_conflict(&c1, &c2);
        assert!(
            result.is_some(),
            "Confidence gap >0.3 on similar topic should trigger ConfidenceMismatch"
        );
        assert_eq!(
            result.unwrap().conflict_type,
            ConflictType::ConfidenceMismatch
        );
    }

    #[test]
    fn test_conflict_d2_source_disagreement() {
        let c1 = make_claim_with_source("the team has 12 engineers working on backend", 0.8, "hr");
        let c2 = make_claim_with_source(
            "the team has 8 engineers working on backend",
            0.8,
            "engineering",
        );
        let result = detect_conflict(&c1, &c2);
        assert!(
            result.is_some(),
            "Different sources, similar content with different numbers should conflict"
        );
    }

    #[test]
    fn test_conflict_d2_temporal_shift() {
        let shared_parent = uuid::Uuid::new_v4();
        let mut c1 = make_claim("Bob is studying at Stanford", 0.9);
        let mut c2 = make_claim("Bob graduated from Stanford and works at Google", 0.85);
        c1.derived_from = vec![shared_parent];
        c2.derived_from = vec![shared_parent];
        let result = detect_conflict(&c1, &c2);
        assert!(
            result.is_some(),
            "Shared derivation + same node_type should trigger TemporalShift"
        );
        assert_eq!(result.unwrap().conflict_type, ConflictType::TemporalShift);
    }

    #[test]
    fn test_conflict_d2_contextual_difference() {
        let mut c1 = EpistemicClaim::new(
            "the deployment process uses blue green strategy with canary checks",
            "tenant-production",
            NodeType::Event,
            Claimant::System,
            AccessEnvelope::new("tenant-production"),
            ProvenanceRecord::new("monitoring".into(), "system".into(), "api".into()),
        );
        c1.confidence = 0.9;

        let mut c2 = EpistemicClaim::new(
            "the deployment process uses rolling strategy with canary checks",
            "tenant-staging",
            NodeType::Event,
            Claimant::System,
            AccessEnvelope::new("tenant-staging"),
            ProvenanceRecord::new("monitoring".into(), "system".into(), "api".into()),
        );
        c2.confidence = 0.9;

        let result = detect_conflict(&c1, &c2);
        assert!(
            result.is_some(),
            "Different tenants with similar content should trigger ContextualDifference"
        );
        assert_eq!(
            result.unwrap().conflict_type,
            ConflictType::ContextualDifference
        );
    }

    #[test]
    fn test_conflict_d2_no_conflict_complementary() {
        let c1 = make_claim_with_source("the API supports JSON and XML formats", 0.9, "docs");
        let c2 = make_claim_with_source(
            "the API rate limit is 1000 requests per minute",
            0.9,
            "docs",
        );
        let result = detect_conflict(&c1, &c2);
        assert!(
            result.is_none(),
            "Complementary facts about different aspects should NOT conflict"
        );
    }

    #[test]
    fn test_conflict_d2_no_conflict_consistent() {
        let c1 = make_claim("Carol enjoys hiking in the mountains", 0.8);
        let c2 = make_claim("Carol went hiking at Yosemite last Saturday", 0.9);
        let result = detect_conflict(&c1, &c2);
        assert!(result.is_none(), "Consistent claims should NOT conflict");
    }

    #[test]
    fn test_conflict_d2_temporal_inconsistency() {
        // Same topic, but valid_start times differ by >30 days
        let mut c1 = make_claim_with_source(
            "the quarterly report shows growth in revenue",
            0.8,
            "finance",
        );
        let mut c2 = make_claim_with_source(
            "the quarterly report shows decline in revenue",
            0.8,
            "finance",
        );
        // Set c1 to 60 days ago
        c1.t_valid_start = chrono::Utc::now() - chrono::Duration::days(60);
        c2.t_valid_start = chrono::Utc::now();
        let result = detect_conflict(&c1, &c2);
        assert!(
            result.is_some(),
            "Claims >30 days apart with similar content should trigger TemporalInconsistency"
        );
        assert_eq!(
            result.unwrap().conflict_type,
            ConflictType::TemporalInconsistency
        );
    }

    #[test]
    fn test_conflict_d2_context_dependent() {
        // Same content, different node_type = meaning depends on context
        let mut c1 = EpistemicClaim::new(
            "the system performance is excellent overall and meets targets",
            "test",
            NodeType::Entity, // Entity context
            Claimant::System,
            AccessEnvelope::new("test"),
            ProvenanceRecord::new("src".into(), "system".into(), "test".into()),
        );
        c1.confidence = 0.8;

        let mut c2 = EpistemicClaim::new(
            "the system performance is excellent overall and exceeds targets",
            "test",
            NodeType::Event, // Event context — different node_type
            Claimant::System,
            AccessEnvelope::new("test"),
            ProvenanceRecord::new("src".into(), "system".into(), "test".into()),
        );
        c2.confidence = 0.8;

        let result = detect_conflict(&c1, &c2);
        assert!(
            result.is_some(),
            "Same content with different node_type should trigger ContextDependent"
        );
        assert_eq!(
            result.unwrap().conflict_type,
            ConflictType::ContextDependent
        );
    }

    #[test]
    fn test_conflict_d2_retracted_claim_ignored() {
        let c1 = make_claim("the server is down", 0.9);
        let mut c2 = make_claim("the server is not down", 0.9);
        c2.epistemic_status = EpistemicStatus::Retracted;
        let result = detect_conflict(&c1, &c2);
        assert!(
            result.is_none(),
            "Retracted claims should be excluded from conflict detection"
        );
    }
}
