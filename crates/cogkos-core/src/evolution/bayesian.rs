use crate::models::EpistemicClaim;

/// Convert probability to log odds
pub fn log_odds(probability: f64) -> f64 {
    if probability <= 0.0 {
        return -10.0; // Cap at reasonable minimum
    }
    if probability >= 1.0 {
        return 10.0; // Cap at reasonable maximum
    }
    (probability / (1.0 - probability)).ln()
}

/// Convert log odds back to probability
pub fn odds_to_prob(log_odds: f64) -> f64 {
    let capped = log_odds.clamp(-10.0, 10.0);
    1.0 / (1.0 + (-capped).exp())
}

/// Bayesian aggregation of multiple claims
/// Combines evidence from independent sources using log odds
pub fn bayesian_aggregate(claims: &[EpistemicClaim]) -> f64 {
    if claims.is_empty() {
        return 0.5; // Uniform prior
    }

    // Start with neutral prior
    let prior_log_odds = 0.0;

    // Sum log odds from all claims (weighted by confidence)
    let total_log_odds: f64 =
        claims.iter().map(|c| log_odds(c.confidence)).sum::<f64>() + prior_log_odds;

    odds_to_prob(total_log_odds)
}

/// Bayesian aggregation with source deduplication
/// Phase 1: source_id based deduplication
pub fn bayesian_aggregate_deduplicated(claims: &[EpistemicClaim]) -> f64 {
    use std::collections::HashSet;

    if claims.is_empty() {
        return 0.5;
    }

    // Group by source_id to deduplicate
    let unique_sources: HashSet<String> = claims
        .iter()
        .map(|c| c.provenance.source_id.clone())
        .collect();

    // Take the highest confidence claim per source
    let mut unique_claims = Vec::with_capacity(unique_sources.len());
    for source_id in unique_sources {
        let best_claim = claims
            .iter()
            .filter(|c| c.provenance.source_id == source_id)
            .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap());

        if let Some(claim) = best_claim {
            unique_claims.push(claim.clone());
        }
    }

    bayesian_aggregate(&unique_claims)
}

/// Calculate confidence from a single claim with self-validation
pub fn calculate_base_confidence(
    source_reliability: f64,
    content_coherence: f64,
    temporal_proximity: f64, // 1.0 = recent, 0.0 = old
) -> f64 {
    // Simple weighted combination
    let confidence = 0.4 * source_reliability + 0.3 * content_coherence + 0.3 * temporal_proximity;

    confidence.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    fn create_test_claim(content: &str, confidence: f64) -> EpistemicClaim {
        let mut claim = EpistemicClaim::new(
            "test-tenant".to_string(),
            content.to_string(),
            NodeType::Entity,
            Claimant::System,
            AccessEnvelope::new("test-tenant"),
            ProvenanceRecord::new("test".to_string(), "test".to_string(), "test".to_string()),
        );
        claim.confidence = confidence;
        claim
    }

    #[test]
    fn test_log_odds_roundtrip() {
        let prob = 0.75;
        let lo = log_odds(prob);
        let back = odds_to_prob(lo);
        assert!((back - prob).abs() < 0.001);
    }

    #[test]
    fn test_bayesian_aggregate_empty() {
        let claims: Vec<EpistemicClaim> = vec![];
        let result = bayesian_aggregate(&claims);
        assert_eq!(result, 0.5);
    }

    #[test]
    fn test_bayesian_aggregate_single() {
        let claim = create_test_claim("test", 0.75);
        let result = bayesian_aggregate(&[claim]);
        assert!((result - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_bayesian_aggregate_multiple() {
        let mut claim1 = create_test_claim("test1", 0.6);
        claim1.confidence = 0.6;
        let mut claim2 = create_test_claim("test2", 0.7);
        claim2.confidence = 0.7;

        let result = bayesian_aggregate(&[claim1, claim2]);
        // Combined confidence should be higher than individual
        assert!(result > 0.7);
        assert!(result <= 1.0);
    }
}
