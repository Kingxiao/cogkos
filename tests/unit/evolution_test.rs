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
            Claimant::Human { user_id: "user1".into(), role: "tester".into() },
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
        assert!(result.is_none(), "Same claim should not conflict with itself");
    }

    #[test]
    fn test_conflict_detection_different_content() {
        let c1 = make_claim("Earth is round", 0.9);
        let c2 = make_claim("Completely different topic about quantum physics", 0.9);
        let result = detect_conflict(&c1, &c2);
        assert!(result.is_none(), "Different content should not conflict");
    }
}
