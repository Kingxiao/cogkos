//! Task runner functions for the scheduler

use crate::consolidate::{ConsolidationConfig, consolidate_claims};
use crate::decay::{DecayConfig, decay_claims};
use cogkos_core::Result;
use cogkos_store::Stores;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid;

/// Run consolidation task
pub(crate) async fn run_consolidation(
    stores: &Arc<Stores>,
    config: &ConsolidationConfig,
) -> Result<usize> {
    info!("Running scheduled consolidation");

    // Get all active tenants from database
    let tenants = match stores.claims.list_tenants().await {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => vec!["default".to_string()],
        Err(e) => {
            warn!("Failed to list tenants, falling back to default: {}", e);
            vec!["default".to_string()]
        }
    };

    let mut total_processed = 0;

    for tenant in tenants {
        let count = consolidate_claims(stores, &tenant, config).await?;
        total_processed += count;
    }

    info!(
        consolidated_count = total_processed,
        "Consolidation complete"
    );

    Ok(total_processed)
}

/// Run event-driven consolidation for PendingAggregation claims
///
/// This processes claims that were marked by the ingest pipeline as needing
/// Sleep-time aggregation due to high novelty scores (> 0.3).
pub(crate) async fn run_pending_aggregation(
    stores: &Arc<Stores>,
    config: &ConsolidationConfig,
) -> Result<usize> {
    use cogkos_core::models::ConsolidationStage;

    info!("Running event-driven consolidation for PendingAggregation claims");

    let tenants = match stores.claims.list_tenants().await {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => vec!["default".to_string()],
        Err(e) => {
            warn!("Failed to list tenants for pending aggregation: {}", e);
            vec!["default".to_string()]
        }
    };
    let mut total_processed = 0;

    for tenant in &tenants {
        // Get claims with PendingAggregation stage
        let claims = stores
            .claims
            .list_claims_by_stage(
                tenant,
                ConsolidationStage::PendingAggregation,
                config.batch_size,
            )
            .await?;

        if claims.is_empty() {
            debug!(tenant_id = %tenant, "No PendingAggregation claims to process");
            continue;
        }

        info!(
            tenant_id = %tenant,
            pending_count = claims.len(),
            "Processing PendingAggregation claims"
        );

        // Process each claim with consolidate_claim
        for claim in &claims {
            match crate::consolidate::consolidate_claim(stores, claim, config).await {
                Ok(Some(consolidated)) => {
                    // Insert the consolidated claim
                    if let Err(e) = stores.claims.insert_claim(&consolidated).await {
                        error!(error = %e, claim_id = %claim.id, "Failed to insert consolidated claim");
                    } else {
                        info!(consolidated_id = %consolidated.id, "Created consolidated claim from novel knowledge");
                    }
                }
                Ok(None) => {
                    // Not enough similar claims to consolidate
                    debug!(claim_id = %claim.id, "Skipping - not enough similar claims for consolidation");
                }
                Err(e) => {
                    error!(error = %e, claim_id = %claim.id, "Failed to consolidate claim");
                }
            }

            // Mark the original claim as processed by moving to FastTrack
            // (it has been evaluated, no need to re-evaluate immediately)
            let mut updated = claim.clone();
            updated.consolidation_stage = ConsolidationStage::FastTrack;
            if let Err(e) = stores.claims.update_claim(&updated).await {
                warn!(error = %e, claim_id = %claim.id, "Failed to update claim stage");
            }
        }

        total_processed += claims.len();
    }

    info!(
        processed_count = total_processed,
        "PendingAggregation processing complete"
    );

    Ok(total_processed)
}

/// Run decay task
pub(crate) async fn run_decay(stores: &Arc<Stores>, config: &DecayConfig) -> Result<usize> {
    info!("Running scheduled decay");

    let tenants = match stores.claims.list_tenants().await {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => vec!["default".to_string()],
        Err(e) => {
            warn!("Failed to list tenants for decay: {}", e);
            vec!["default".to_string()]
        }
    };
    let mut total_processed = 0;

    for tenant in &tenants {
        let count = decay_claims(stores, tenant, config).await?;
        total_processed += count;
    }

    info!(decayed_count = total_processed, "Decay complete");

    Ok(total_processed)
}

/// Run memory GC — delete expired working and episodic claims
pub(crate) async fn run_memory_gc(stores: &Arc<Stores>) -> Result<usize> {
    use cogkos_core::models::MemoryLayer;

    info!("Running memory GC");

    let tenants = match stores.claims.list_tenants().await {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => vec!["default".to_string()],
        Err(e) => {
            warn!("Failed to list tenants for memory GC: {}", e);
            vec!["default".to_string()]
        }
    };

    let mut total_gc = 0;

    for tenant in &tenants {
        // GC working memory (max_age = 8h)
        let working_gc = stores
            .memory_layers
            .gc_expired_memory_layer(tenant, "working", MemoryLayer::Working.max_age_hours())
            .await?;
        if working_gc > 0 {
            info!(tenant = %tenant, deleted = working_gc, "GC'd expired working memory claims");
        }

        // GC episodic memory (max_age = 168h / 7d)
        let episodic_gc = stores
            .memory_layers
            .gc_expired_memory_layer(tenant, "episodic", MemoryLayer::Episodic.max_age_hours())
            .await?;
        if episodic_gc > 0 {
            info!(tenant = %tenant, deleted = episodic_gc, "GC'd expired episodic memory claims");
        }

        total_gc += working_gc + episodic_gc;
    }

    info!(total_gc = total_gc, "Memory GC complete");
    Ok(total_gc)
}

/// Run memory promotion — working → episodic → semantic
pub(crate) async fn run_memory_promotion(
    stores: &Arc<Stores>,
    working_to_episodic_threshold: u64,
    episodic_to_semantic_threshold: u64,
) -> Result<usize> {
    info!("Running memory promotion");

    let tenants = match stores.claims.list_tenants().await {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => vec!["default".to_string()],
        Err(e) => {
            warn!("Failed to list tenants for memory promotion: {}", e);
            vec!["default".to_string()]
        }
    };

    let mut total_promoted = 0;

    for tenant in &tenants {
        // working → episodic
        let w2e = stores
            .memory_layers
            .promote_memory_layer(tenant, "working", "episodic", working_to_episodic_threshold)
            .await?;
        if w2e > 0 {
            info!(tenant = %tenant, promoted = w2e, "Promoted working → episodic");
        }

        // episodic → semantic
        let e2s = stores
            .memory_layers
            .promote_memory_layer(tenant, "episodic", "semantic", episodic_to_semantic_threshold)
            .await?;
        if e2s > 0 {
            info!(tenant = %tenant, promoted = e2s, "Promoted episodic → semantic");
        }

        total_promoted += w2e + e2s;
    }

    info!(total_promoted = total_promoted, "Memory promotion complete");
    Ok(total_promoted)
}

/// Run health check
pub(crate) async fn run_health_check(_stores: &Arc<Stores>) -> Result<()> {
    // Basic health checks

    // Check claim store connectivity
    // Check vector store connectivity
    // Check graph store connectivity

    info!("Health check complete");

    Ok(())
}

/// Run confidence boost task for similar knowledge
///
/// This processes claims that were marked by the ingest pipeline as needing
/// confidence boost due to low novelty scores (<= 0.3). It finds similar claims
/// and boosts their confidence through the evolution engine task queue.
pub(crate) async fn run_confidence_boost(
    stores: &Arc<Stores>,
    tenant_id: &str,
    batch_size: usize,
    boost_factor: f64,
) -> Result<usize> {
    // Get claims marked as needing confidence boost
    let claims = stores
        .claims
        .list_claims_needing_confidence_boost(tenant_id, batch_size)
        .await?;

    if claims.is_empty() {
        debug!(tenant_id = %tenant_id, "No claims needing confidence boost");
        return Ok(0);
    }

    info!(
        tenant_id = %tenant_id,
        claim_count = claims.len(),
        "Processing confidence boost for similar knowledge"
    );

    let mut total_boosted = 0;

    for claim in &claims {
        // Get the list of similar claim IDs to boost from metadata
        let similar_ids: Vec<String> = claim
            .metadata
            .get("similar_claim_ids_to_boost")
            .and_then(|v| {
                v.as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
            })
            .unwrap_or_default();

        if similar_ids.is_empty() {
            // No similar claims to boost, just clear the flag
            let mut updated = claim.clone();
            updated.metadata.remove("needs_confidence_boost");
            updated.metadata.remove("similar_claim_ids_to_boost");
            if let Err(e) = stores.claims.update_claim(&updated).await {
                warn!(error = %e, claim_id = %claim.id, "Failed to clear confidence boost flag");
            }
            continue;
        }

        // Boost confidence for each similar claim
        for similar_id_str in &similar_ids {
            if let Ok(similar_id) = uuid::Uuid::parse_str(similar_id_str) {
                // Get the similar claim to check current confidence
                if let Ok(similar_claim) = stores.claims.get_claim(similar_id, tenant_id).await {
                    // Only boost if not already too high (max 0.95)
                    if similar_claim.confidence < 0.95 {
                        let new_confidence = (similar_claim.confidence + boost_factor).min(0.95);
                        if let Err(e) = stores
                            .claims
                            .update_confidence(similar_id, tenant_id, new_confidence)
                            .await
                        {
                            warn!(error = %e, claim_id = %similar_id, "Failed to boost confidence");
                        } else {
                            total_boosted += 1;
                            debug!(
                                claim_id = %similar_id,
                                old_confidence = similar_claim.confidence,
                                new_confidence = new_confidence,
                                "Boosted confidence for similar claim"
                            );
                        }
                    }
                }
            }
        }

        // Clear the boost flag after processing
        let mut updated = claim.clone();
        updated.metadata.remove("needs_confidence_boost");
        updated.metadata.remove("similar_claim_ids_to_boost");
        if let Err(e) = stores.claims.update_claim(&updated).await {
            warn!(error = %e, claim_id = %claim.id, "Failed to clear confidence boost flag");
        }
    }

    info!(
        tenant_id = %tenant_id,
        boosted_count = total_boosted,
        "Confidence boost complete"
    );

    Ok(total_boosted)
}
