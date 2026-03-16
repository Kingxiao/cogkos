//! Knowledge decay task
//!
//! Implements time-based confidence decay for unconfirmed or unused knowledge.
//! Formula: effective_confidence(t) = confidence × e^(-λt)
//! Modified by activation_weight to slow decay for frequently accessed knowledge.
//!
//! Mixed Decay Model:
//! - Business knowledge: StepFunction (version replacement, no natural decay)
//! - Experiential/Learned knowledge: ExponentialDecay

use cogkos_core::Result;
use cogkos_core::evolution::decay::{
    calculate_decay, calculate_decay_with_revalidation, calculate_effective_durability,
    needs_revalidation,
};
use cogkos_store::Stores;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Mixed decay model based on knowledge type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DecayModel {
    /// Business knowledge: version replacement, no natural decay
    StepFunction {
        valid_until: Option<chrono::DateTime<chrono::Utc>>,
        post_expiry_confidence: f64,
    },
    /// Experiential/Learned knowledge: exponential decay
    ExponentialDecay {
        half_life_hours: f64,
        min_confidence: f64,
    },
}

impl Default for DecayModel {
    fn default() -> Self {
        DecayModel::ExponentialDecay {
            half_life_hours: 720.0, // 30 days
            min_confidence: 0.1,
        }
    }
}

/// Configuration for knowledge decay
#[derive(Debug, Clone)]
pub struct DecayConfig {
    /// Base decay rate (λ) - fraction lost per hour
    pub lambda: f64,
    /// Confidence threshold below which claim needs revalidation
    pub revalidation_threshold: f64,
    /// Maximum age (hours) before mandatory revalidation
    pub max_age_hours: f64,
    /// Minimum activation weight (prevents infinite persistence)
    pub min_activation_weight: f64,
    /// Maximum claims to process per batch
    pub batch_size: usize,
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            lambda: 0.01, // 1% per hour base rate
            revalidation_threshold: 0.3,
            max_age_hours: 24.0 * 30.0, // 30 days
            min_activation_weight: 0.1,
            batch_size: 1000,
        }
    }
}

/// Run decay on claims
///
/// This is a periodic task (daily) that applies time-based decay
/// to knowledge that hasn't been confirmed or accessed.
pub async fn decay_claims(
    stores: &Arc<Stores>,
    tenant_id: &str,
    config: &DecayConfig,
) -> Result<usize> {
    info!(tenant_id = %tenant_id, "Starting knowledge decay");

    // Get claims that need decay calculation
    let claims = stores
        .claims
        .list_claims_needing_revalidation(
            tenant_id,
            config.revalidation_threshold,
            config.batch_size,
        )
        .await?;

    if claims.is_empty() {
        debug!(tenant_id = %tenant_id, "No claims needing decay");
        return Ok(0);
    }

    info!(
        tenant_id = %tenant_id,
        claims_to_decay = claims.len(),
        "Processing decay for claims"
    );

    let now = chrono::Utc::now();
    let mut decayed_count = 0;

    for claim in claims {
        // Calculate hours since last access (or creation if never accessed)
        let hours_since_access = claim
            .last_accessed
            .map(|la| (now - la).num_hours() as f64)
            .unwrap_or((now - claim.created_at).num_hours() as f64);

        // Calculate effective durability based on usage
        let effective_durability = calculate_effective_durability(
            claim.durability,
            claim.access_count,
            0, // confirmation_count - would come from feedback
        );

        // Apply decay with adjusted lambda based on durability
        let adjusted_lambda = config.lambda * (1.0 - effective_durability * 0.5);

        // Ensure minimum activation weight
        let activation = claim.activation_weight.max(config.min_activation_weight);

        // Calculate new confidence
        let new_confidence = calculate_decay(
            claim.confidence,
            adjusted_lambda,
            hours_since_access,
            activation,
        );

        // Check if revalidation is needed
        let needs_rev = needs_revalidation(
            new_confidence,
            config.revalidation_threshold,
            hours_since_access,
            config.max_age_hours,
        );

        if new_confidence != claim.confidence {
            debug!(
                claim_id = %claim.id,
                old_confidence = claim.confidence,
                new_confidence = new_confidence,
                hours_elapsed = hours_since_access,
                "Decaying claim confidence"
            );

            if let Err(e) = stores
                .claims
                .update_confidence(claim.id, new_confidence)
                .await
            {
                warn!(error = %e, claim_id = %claim.id, "Failed to update confidence");
            }
        }

        if needs_rev {
            debug!(claim_id = %claim.id, "Claim needs revalidation");
            // NOTE: Requires store implementation
        }

        decayed_count += 1;
    }

    info!(
        tenant_id = %tenant_id,
        decayed_count = decayed_count,
        "Decay complete"
    );

    Ok(decayed_count)
}

/// Apply revalidation boost to a claim
///
/// Called when new evidence confirms an existing claim
pub async fn revalidate_claim(
    stores: &Arc<Stores>,
    claim_id: uuid::Uuid,
    tenant_id: &str,
    boost: f64,
) -> Result<f64> {
    info!(claim_id = %claim_id, boost = boost, "Applying revalidation boost");

    let claim = stores.claims.get_claim(claim_id, tenant_id).await?;

    let now = chrono::Utc::now();
    let hours_since_access = claim
        .last_accessed
        .map(|la| (now - la).num_hours() as f64)
        .unwrap_or(0.0);

    let new_confidence = calculate_decay_with_revalidation(
        claim.confidence,
        0.01, // base_lambda
        hours_since_access,
        claim.activation_weight,
        boost,
    );

    stores
        .claims
        .update_confidence(claim_id, new_confidence)
        .await?;

    Ok(new_confidence)
}

/// Calculate confidence with custom parameters
///
/// Exposes the decay formula for external use
pub fn calculate_confidence_decay(
    confidence: f64,
    lambda: f64,
    time_delta_hours: f64,
    activation_weight: f64,
) -> f64 {
    calculate_decay(confidence, lambda, time_delta_hours, activation_weight)
}

/// Calculate decay with revalidation
pub fn calculate_confidence_decay_with_boost(
    confidence: f64,
    lambda: f64,
    time_delta_hours: f64,
    activation_weight: f64,
    revalidation_boost: f64,
) -> f64 {
    calculate_decay_with_revalidation(
        confidence,
        lambda,
        time_delta_hours,
        activation_weight,
        revalidation_boost,
    )
}

/// Calculate remaining time until confidence drops to threshold
///
/// Inverse of decay formula:
/// t = -ln(P/P0) / λ * activation_weight
pub fn time_to_threshold(
    current_confidence: f64,
    target_threshold: f64,
    lambda: f64,
    activation_weight: f64,
) -> f64 {
    if current_confidence <= target_threshold || lambda <= 0.0 {
        return 0.0;
    }

    let effective_lambda = lambda / activation_weight.max(0.1);
    let ratio = current_confidence / target_threshold;

    if ratio <= 1.0 {
        return 0.0;
    }

    let hours = ratio.ln() / effective_lambda;
    hours.max(0.0)
}

/// Batch decay for multiple tenants
pub async fn decay_all_tenants(
    _stores: &Arc<Stores>,
    _config: &DecayConfig,
) -> Result<HashMap<String, usize>> {
    info!("Running decay for all tenants");

    // In a real implementation, we'd get list of tenants from store
    // For now, return empty
    let results = HashMap::new();

    // NOTE: Requires store implementation
    // let tenants = stores.get_active_tenants().await?;
    // for tenant in tenants {
    //     let count = decay_claims(stores, &tenant, config).await?;
    //     results.insert(tenant, count);
    // }

    Ok(results)
}

use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;
    use cogkos_core::models::{
        AccessEnvelope, Claimant, EpistemicClaim, NodeType, ProvenanceRecord,
    };

    fn create_test_claim(
        content: &str,
        confidence: f64,
        activation: f64,
        hours_ago: i64,
    ) -> EpistemicClaim {
        let prov = ProvenanceRecord {
            source_id: "test".to_string(),
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
        claim.activation_weight = activation;

        if hours_ago > 0 {
            claim.last_accessed = Some(chrono::Utc::now() - chrono::Duration::hours(hours_ago));
        }

        claim
    }

    #[test]
    fn test_time_to_threshold() {
        let hours = time_to_threshold(0.8, 0.5, 0.01, 0.5);
        assert!(hours > 0.0);
    }

    #[test]
    fn test_decay_preserves_high_confidence_with_high_activation() {
        let claim = create_test_claim("test", 0.9, 1.0, 24);

        let new_conf =
            calculate_confidence_decay(claim.confidence, 0.01, 24.0, claim.activation_weight);

        // High activation should slow decay - new confidence should be less than original
        // but decay should be less than with low activation
        assert!(new_conf < claim.confidence); // It should decay
        assert!(new_conf > 0.6); // But not too much with high activation
    }

    #[test]
    fn test_decay_affects_low_activation_claims_faster() {
        let claim_low = create_test_claim("test", 0.8, 0.1, 24);
        let claim_high = create_test_claim("test", 0.8, 1.0, 24);

        let new_low = calculate_confidence_decay(
            claim_low.confidence,
            0.01,
            24.0,
            claim_low.activation_weight,
        );
        let new_high = calculate_confidence_decay(
            claim_high.confidence,
            0.01,
            24.0,
            claim_high.activation_weight,
        );

        assert!(new_low < new_high);
    }
}
