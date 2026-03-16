//! Bayesian consolidation task
//!
//! Aggregates multiple assertions from different sources into a single belief.
//! Uses Bayesian inference to combine evidence while handling source independence.

use cogkos_core::Result;
use cogkos_core::evolution::bayesian::bayesian_aggregate_deduplicated;
use cogkos_core::evolution::{log_odds, odds_to_prob};
use cogkos_core::models::{ConsolidationStage, EpistemicClaim, EpistemicStatus};
use cogkos_store::Stores;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Configuration for consolidation
#[derive(Debug, Clone)]
pub struct ConsolidationConfig {
    /// Minimum number of sources required for consolidation
    pub min_sources: usize,
    /// Confidence threshold to promote to Consolidated stage
    pub promotion_threshold: f64,
    /// Maximum claims to process in one batch
    pub batch_size: usize,
    /// Base decay rate for old claims
    pub base_lambda: f64,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            min_sources: 2,
            promotion_threshold: 0.7,
            batch_size: 100,
            base_lambda: 0.01,
        }
    }
}

/// Run consolidation on FastTrack claims
///
/// This is a periodic task that runs every few hours to aggregate
/// multiple assertions about the same entity into consolidated beliefs.
pub async fn consolidate_claims(
    stores: &Arc<Stores>,
    tenant_id: &str,
    config: &ConsolidationConfig,
) -> Result<usize> {
    info!(tenant_id = %tenant_id, "Starting claim consolidation");

    // Get FastTrack claims that might benefit from consolidation
    let claims = stores
        .claims
        .list_claims_by_stage(tenant_id, ConsolidationStage::FastTrack, config.batch_size)
        .await?;

    if claims.is_empty() {
        debug!(tenant_id = %tenant_id, "No FastTrack claims to consolidate");
        return Ok(0);
    }

    info!(
        tenant_id = %tenant_id,
        fasttrack_claims = claims.len(),
        "Processing FastTrack claims"
    );

    // Group claims by semantic similarity (content-based for now)
    let mut groups: HashMap<String, Vec<EpistemicClaim>> = HashMap::new();

    for claim in claims {
        // Use first few words as simple grouping key
        // Phase 3+: Use embeddings for semantic clustering
        let key = extract_group_key(&claim.content);
        groups.entry(key).or_default().push(claim);
    }

    let mut consolidated_count = 0;

    // Process each group
    for (key, group_claims) in groups {
        if group_claims.len() < config.min_sources {
            debug!(key = %key, claims = group_claims.len(), "Skipping - not enough sources");
            continue;
        }

        // Bayesian aggregation with source deduplication
        let aggregated_confidence = bayesian_aggregate_deduplicated(&group_claims);

        info!(
            key = %key,
            source_count = group_claims.len(),
            aggregated_confidence = aggregated_confidence,
            "Aggregated claims"
        );

        // Determine new status based on confidence
        let _new_status = if aggregated_confidence >= config.promotion_threshold {
            EpistemicStatus::Corroborated
        } else {
            EpistemicStatus::Asserted
        };

        // Update all claims in the group
        // In a real implementation, we might create a new "Consolidated" claim
        // and mark the originals as derived_from
        for claim in &group_claims {
            // Update confidence to aggregated value
            if let Err(e) = stores
                .claims
                .update_confidence(claim.id, aggregated_confidence)
                .await
            {
                warn!(error = %e, claim_id = %claim.id, "Failed to update confidence");
            }

            // NOTE: Update epistemic_status when store supports it
            // NOTE: Update consolidation_stage when confidence is high enough
        }

        consolidated_count += group_claims.len();
    }

    info!(
        tenant_id = %tenant_id,
        consolidated_count = consolidated_count,
        "Consolidation complete"
    );

    Ok(consolidated_count)
}

/// Consolidate a single claim with similar claims
///
/// Used for on-demand consolidation when a new claim is written
pub async fn consolidate_claim(
    stores: &Arc<Stores>,
    claim: &EpistemicClaim,
    config: &ConsolidationConfig,
) -> Result<Option<EpistemicClaim>> {
    debug!(claim_id = %claim.id, "Running on-demand consolidation");

    // Find similar claims
    let similar = stores
        .claims
        .search_claims(&claim.tenant_id, &claim.content, 20)
        .await?;

    // Filter to valid claims (not retracted/superseded)
    let valid_similar: Vec<_> = similar
        .into_iter()
        .filter(|c| {
            c.id != claim.id
                && !matches!(
                    c.epistemic_status,
                    EpistemicStatus::Retracted | EpistemicStatus::Superseded
                )
        })
        .collect();

    if valid_similar.len() < config.min_sources {
        return Ok(None);
    }

    // Include the original claim in aggregation
    let mut all_claims = vec![claim.clone()];
    all_claims.extend(valid_similar);

    // Bayesian aggregation
    let aggregated_confidence = bayesian_aggregate_deduplicated(&all_claims);

    // Create a new consolidated claim
    let mut consolidated = claim.clone();
    consolidated.id = uuid::Uuid::new_v4();
    consolidated.confidence = aggregated_confidence;
    consolidated.consolidation_stage = ConsolidationStage::Consolidated;
    consolidated.derived_from = all_claims.iter().map(|c| c.id).collect();

    if aggregated_confidence >= config.promotion_threshold {
        consolidated.epistemic_status = EpistemicStatus::Corroborated;
    }

    Ok(Some(consolidated))
}

/// Extract grouping key from claim content
///
/// Simple implementation using key terms
/// Phase 3+: Use embeddings for semantic clustering
fn extract_group_key(content: &str) -> String {
    let words: Vec<&str> = content.split_whitespace().take(5).collect();
    words.join(" ").to_lowercase()
}

/// Calculate the effective confidence from multiple sources
///
/// This is the core Bayesian aggregation formula:
/// 1. Convert each confidence to log odds
/// 2. Sum log odds (weighting by source independence)
/// 3. Convert back to probability
///
/// Formula: P(A) = 1 / (1 + e^(-Σlogodds(ci)))
pub fn bayesian_aggregate_with_weights(claims: &[EpistemicClaim], source_weights: &[f64]) -> f64 {
    if claims.is_empty() || source_weights.is_empty() {
        return 0.5;
    }

    if claims.len() != source_weights.len() {
        warn!("Claims and weights length mismatch, using equal weights");
    }

    let prior_log_odds = 0.0; // Neutral prior

    let total: f64 = claims
        .iter()
        .zip(source_weights.iter().cycle())
        .map(|(c, w)| log_odds(c.confidence) * w)
        .sum();

    odds_to_prob(total + prior_log_odds)
}

/// Count independent sources in a set of claims
///
/// Phase 1: Simple source_id deduplication
/// Phase 3+: Track provenance chains for better independence assessment
pub fn count_independent_sources(claims: &[EpistemicClaim]) -> usize {
    use std::collections::HashSet;

    claims
        .iter()
        .map(|c| c.provenance.source_id.clone())
        .collect::<HashSet<_>>()
        .len()
}

/// Get source diversity score (Shannon entropy)
pub fn calculate_source_diversity(claims: &[EpistemicClaim]) -> f64 {
    use std::collections::HashMap;

    if claims.is_empty() {
        return 0.0;
    }

    // Count occurrences of each source
    let mut source_counts: HashMap<&str, usize> = HashMap::new();
    for claim in claims {
        *source_counts
            .entry(&claim.provenance.source_id)
            .or_insert(0) += 1;
    }

    let total = claims.len() as f64;

    // Calculate Shannon entropy
    let entropy: f64 = source_counts
        .values()
        .map(|&count| {
            let p = count as f64 / total;
            if p > 0.0 { -p * p.log2() } else { 0.0 }
        })
        .sum();

    // Normalize to 0-1 range (max entropy = log2(number of sources))
    let max_entropy = (source_counts.len() as f64).log2();
    if max_entropy > 0.0 {
        entropy / max_entropy
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cogkos_core::models::{AccessEnvelope, Claimant, NodeType, ProvenanceRecord};

    fn create_test_claim(content: &str, confidence: f64, source_id: &str) -> EpistemicClaim {
        let prov = ProvenanceRecord {
            source_id: source_id.to_string(),
            source_type: "test".to_string(),
            ingestion_method: "test".to_string(),
            original_url: None,
            audit_hash: "test".to_string(),
        };
        let mut claim = EpistemicClaim::new(
            "test-tenant".to_string(),
            content.to_string(),
            NodeType::Entity,
            Claimant::System,
            AccessEnvelope::new("test-tenant"),
            prov,
        );
        claim.confidence = confidence;
        claim
    }

    #[test]
    fn test_bayesian_aggregate_with_weights() {
        let claims = vec![
            create_test_claim("test", 0.6, "source1"),
            create_test_claim("test", 0.7, "source2"),
        ];
        let weights = vec![1.0, 1.0];

        let result = bayesian_aggregate_with_weights(&claims, &weights);
        assert!(result > 0.6);
    }

    #[test]
    fn test_count_independent_sources() {
        let claims = vec![
            create_test_claim("test", 0.5, "source1"),
            create_test_claim("test", 0.5, "source1"),
            create_test_claim("test", 0.5, "source2"),
        ];

        assert_eq!(count_independent_sources(&claims), 2);
    }

    #[test]
    fn test_calculate_source_diversity() {
        let claims = vec![
            create_test_claim("test", 0.5, "source1"),
            create_test_claim("test", 0.5, "source2"),
            create_test_claim("test", 0.5, "source3"),
        ];

        let diversity = calculate_source_diversity(&claims);
        assert!(diversity > 0.0);
        assert!(diversity <= 1.0);
    }

    #[test]
    fn test_extract_group_key() {
        let key = extract_group_key("The quick brown fox jumps over");
        assert!(key.contains("quick"));
        assert!(key.contains("brown"));
    }
}
