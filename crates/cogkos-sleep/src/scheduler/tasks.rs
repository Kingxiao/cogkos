//! Task runner functions for the scheduler

use crate::consolidate::{ConsolidationConfig, consolidate_claims};
use crate::decay::{DecayConfig, decay_claims};
use cogkos_core::Result;
use cogkos_store::Stores;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid;

use super::SchedulerConfig;

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

/// Run memory GC — delete expired working/episodic claims + hard-delete dead semantic claims
pub(crate) async fn run_memory_gc(stores: &Arc<Stores>) -> Result<usize> {
    use cogkos_core::authority::AuthorityTier;
    use cogkos_core::models::{EpistemicStatus, MemoryLayer};

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

        // Hard GC: physically delete dead semantic claims
        // Criteria: Retracted status, confidence < 0.05, not accessed for 30+ days,
        //           and NOT T1/T2 authority (never delete canonical/curated).
        let mut hard_deleted = 0usize;
        let now = chrono::Utc::now();
        if let Ok(all_claims) = stores.claims.query_claims(tenant, &[]).await {
            for claim in &all_claims {
                let tier = AuthorityTier::resolve(claim);
                let not_accessed_30d = claim
                    .last_accessed
                    .map_or(true, |t| (now - t).num_days() > 30);
                let should_hard_delete = claim.confidence < 0.05
                    && not_accessed_30d
                    && !matches!(tier, AuthorityTier::Canonical | AuthorityTier::Curated)
                    && matches!(claim.epistemic_status, EpistemicStatus::Retracted);

                if should_hard_delete {
                    stores.claims.delete_claim(claim.id, tenant).await.ok();
                    hard_deleted += 1;
                }
            }
        }
        if hard_deleted > 0 {
            info!(tenant = %tenant, deleted = hard_deleted, "Hard-deleted dead semantic claims");
        }

        total_gc += working_gc + episodic_gc + hard_deleted;
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
            .promote_memory_layer(
                tenant,
                "episodic",
                "semantic",
                episodic_to_semantic_threshold,
            )
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

/// Run insight extraction from unresolved conflicts
///
/// For each tenant, fetches unresolved conflicts, collects related claims,
/// and uses InsightExtractor to identify higher-level insights. Each insight
/// is elevated to a new EpistemicClaim with NodeType::Insight.
pub(crate) async fn run_insight_extraction(
    stores: &Arc<Stores>,
    batch_size: usize,
) -> Result<usize> {
    use cogkos_core::evolution::insight_extraction::{
        InsightExtractionConfig, InsightExtractor, elevate_insight_to_claim,
    };
    use cogkos_core::models::ResolutionStatus;
    use std::collections::HashMap;

    let start = std::time::Instant::now();
    info!("Running insight extraction");

    let tenants = match stores.claims.list_tenants().await {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => vec!["default".to_string()],
        Err(e) => {
            warn!("Failed to list tenants for insight extraction: {}", e);
            vec!["default".to_string()]
        }
    };

    let mut total_insights = 0;
    let extractor = InsightExtractor::new(InsightExtractionConfig::default());

    for tenant in &tenants {
        let conflicts = match stores
            .claims
            .list_unresolved_conflicts(tenant, batch_size)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                warn!(tenant = %tenant, error = %e, "Failed to list unresolved conflicts");
                continue;
            }
        };

        if conflicts.is_empty() {
            debug!(tenant = %tenant, "No unresolved conflicts for insight extraction");
            continue;
        }

        // Collect all unique claim IDs from conflicts
        let mut claim_ids = std::collections::HashSet::new();
        for conflict in &conflicts {
            claim_ids.insert(conflict.claim_a_id);
            claim_ids.insert(conflict.claim_b_id);
        }

        // Fetch claims
        let mut claims_map: HashMap<uuid::Uuid, cogkos_core::models::EpistemicClaim> =
            HashMap::new();
        for claim_id in &claim_ids {
            match stores.claims.get_claim(*claim_id, tenant).await {
                Ok(claim) => {
                    claims_map.insert(*claim_id, claim);
                }
                Err(e) => {
                    debug!(claim_id = %claim_id, error = %e, "Skipping missing claim");
                }
            }
        }

        if claims_map.len() < 2 {
            debug!(tenant = %tenant, "Not enough claims for insight extraction");
            continue;
        }

        // Check preconditions and extract
        if !extractor.should_extract_insights(&conflicts, claims_map.len()) {
            debug!(
                tenant = %tenant,
                conflict_count = conflicts.len(),
                claim_count = claims_map.len(),
                "Conditions not met for insight extraction"
            );
            continue;
        }

        let insights = extractor.extract_insights(&conflicts, &claims_map);

        if insights.is_empty() {
            debug!(tenant = %tenant, "No insights extracted");
            continue;
        }

        info!(
            tenant = %tenant,
            insight_count = insights.len(),
            "Extracted insights from conflicts"
        );

        for insight in &insights {
            let claim = elevate_insight_to_claim(insight, tenant);
            if let Err(e) = stores.claims.insert_claim(&claim).await {
                error!(error = %e, insight_id = %insight.id, "Failed to insert insight claim");
                continue;
            }

            // Mark source conflicts as Elevated
            for conflict_id in &insight.source_conflicts {
                if let Err(e) = stores
                    .claims
                    .resolve_conflict(
                        *conflict_id,
                        tenant,
                        ResolutionStatus::Elevated,
                        Some(format!("Elevated to insight claim {}", claim.id)),
                    )
                    .await
                {
                    warn!(error = %e, conflict_id = %conflict_id, "Failed to mark conflict as elevated");
                }
            }

            total_insights += 1;
        }
    }

    cogkos_core::monitoring::METRICS
        .record_duration("cogkos_scheduler_task_duration_seconds", start.elapsed());
    info!(
        total_insights = total_insights,
        duration_ms = start.elapsed().as_millis() as u64,
        "Insight extraction complete"
    );
    Ok(total_insights)
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

/// Run prediction validation — correlate feedback with cached predictions,
/// compute prediction_error, and write back to claims.
///
/// Strategy:
/// 1. For each tenant, get recent query hashes that received feedback
/// 2. For each query_hash, retrieve the cached response (if any)
/// 3. Extract best_belief claim_ids from the cached response
/// 4. Compute prediction_error = |confidence - feedback_success_rate|
/// 5. Write back last_prediction_error to each claim
/// 6. Optionally record to PredictionHistoryStore
pub(crate) async fn run_prediction_validation(
    stores: &Arc<Stores>,
    config: &SchedulerConfig,
    prediction_history: Option<&Arc<dyn cogkos_store::PredictionHistoryStore>>,
) -> Result<usize> {
    let start = std::time::Instant::now();
    info!("Running prediction validation");

    let tenants = match stores.claims.list_tenants().await {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => vec!["default".to_string()],
        Err(e) => {
            warn!("Failed to list tenants for prediction validation: {}", e);
            vec!["default".to_string()]
        }
    };

    let batch_size = config.prediction_validation_batch_size;
    let mut total_validated = 0;

    for tenant_id in &tenants {
        // Step 1: Get recent query hashes with feedback
        let query_hashes = match stores
            .feedback
            .list_recent_feedback_hashes(tenant_id, batch_size)
            .await
        {
            Ok(h) => h,
            Err(e) => {
                warn!(tenant = %tenant_id, error = %e, "Failed to list feedback hashes");
                continue;
            }
        };

        if query_hashes.is_empty() {
            debug!(tenant = %tenant_id, "No feedback query hashes to validate");
            continue;
        }

        for query_hash in &query_hashes {
            // Step 2: Get cached response for this query hash
            let cache_entry = match stores.cache.get_cached(tenant_id, *query_hash).await {
                Ok(Some(entry)) => entry,
                Ok(None) => {
                    debug!(query_hash = %query_hash, "No cached response for query");
                    continue;
                }
                Err(e) => {
                    debug!(query_hash = %query_hash, error = %e, "Failed to get cache entry");
                    continue;
                }
            };

            // Step 3: Extract claim_ids from best_belief
            let claim_ids: Vec<uuid::Uuid> = cache_entry
                .response
                .best_belief
                .as_ref()
                .map(|bb| {
                    let mut ids = bb.claim_ids.clone();
                    if let Some(cid) = bb.claim_id.filter(|c| !ids.contains(c)) {
                        ids.push(cid);
                    }
                    ids
                })
                .unwrap_or_default();

            if claim_ids.is_empty() {
                continue;
            }

            // Get feedback for this query
            let feedbacks = match stores
                .feedback
                .get_feedback_for_query(tenant_id, *query_hash)
                .await
            {
                Ok(f) => f,
                Err(e) => {
                    debug!(query_hash = %query_hash, error = %e, "Failed to get feedback");
                    continue;
                }
            };

            if feedbacks.is_empty() {
                continue;
            }

            // Step 4: Compute feedback success rate
            let success_count = feedbacks.iter().filter(|f| f.success).count() as f64;
            let total_count = feedbacks.len() as f64;
            let success_rate = success_count / total_count;

            // Step 5: Update each referenced claim
            let cached_confidence = cache_entry.confidence;
            let prediction_error = (cached_confidence - success_rate).abs();

            for claim_id in &claim_ids {
                match stores.claims.get_claim(*claim_id, tenant_id).await {
                    Ok(mut claim) => {
                        // Only update if not already validated or if error has changed
                        if claim
                            .last_prediction_error
                            .is_none_or(|prev| (prev - prediction_error).abs() > 0.01)
                        {
                            claim.last_prediction_error = Some(prediction_error);
                            if let Err(e) = stores.claims.update_claim(&claim).await {
                                warn!(
                                    error = %e, claim_id = %claim_id,
                                    "Failed to write back prediction error"
                                );
                            } else {
                                total_validated += 1;
                                debug!(
                                    claim_id = %claim_id,
                                    prediction_error = prediction_error,
                                    cached_confidence = cached_confidence,
                                    success_rate = success_rate,
                                    "Updated claim prediction error"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        debug!(claim_id = %claim_id, error = %e, "Claim not found for validation");
                    }
                }

                // Step 6: Record to PredictionHistoryStore if available
                if let Some(pred_store) = prediction_history {
                    let record = cogkos_store::PredictionErrorRecord {
                        record_id: uuid::Uuid::new_v4().to_string(),
                        tenant_id: tenant_id.clone(),
                        claim_id: claim_id.to_string(),
                        validation_id: format!("pv-{}", query_hash),
                        predicted_probability: cached_confidence,
                        actual_result: success_rate,
                        prediction_error,
                        squared_error: prediction_error * prediction_error,
                        confidence_adjustment: 0.0,
                        predicted_at: cache_entry.created_at.timestamp(),
                        validated_at: Some(chrono::Utc::now().timestamp()),
                        feedback_source: "agent_feedback".to_string(),
                        claim_content: None,
                        claim_type: "cached_prediction".to_string(),
                    };
                    if let Err(e) = pred_store.record_prediction(&record).await {
                        warn!(error = %e, "Failed to record prediction history");
                    }
                }
            }
        }
    }

    cogkos_core::monitoring::METRICS
        .record_duration("cogkos_scheduler_task_duration_seconds", start.elapsed());
    info!(
        total_validated = total_validated,
        duration_ms = start.elapsed().as_millis() as u64,
        "Prediction validation complete"
    );
    Ok(total_validated)
}
