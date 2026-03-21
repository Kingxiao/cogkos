//! Phase 3 task runners: paradigm shift check + framework health monitoring

use cogkos_core::Result;
use cogkos_store::Stores;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::SchedulerConfig;

/// Run paradigm shift anomaly check
///
/// For each tenant, fetches recent claims and computes anomaly signals:
/// - prediction_error_streak: proportion of claims with high prediction_error
/// - conflict_density: contested claims / total claims
/// - calibration_drift: high-confidence claims with high prediction_error
///
/// If anomaly_score >= 0.8, logs a warning (actual paradigm shift execution
/// requires LLM sandbox verification — detection only in this phase).
pub(crate) async fn run_paradigm_shift_check(
    stores: &Arc<Stores>,
    _config: &SchedulerConfig,
) -> Result<usize> {
    use cogkos_core::evolution::paradigm_shift::anomaly::{AnomalyDetector, AnomalyRecommendation};

    let start = std::time::Instant::now();
    info!("Running paradigm shift anomaly check");

    let tenants = match stores.claims.list_tenants().await {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => vec!["default".to_string()],
        Err(e) => {
            warn!("Failed to list tenants for paradigm shift check: {}", e);
            vec!["default".to_string()]
        }
    };

    let mut tenants_checked = 0;

    for tenant in &tenants {
        // Fetch recent claims (last batch, up to 500)
        let claims = match stores
            .claims
            .list_claims_by_stage(
                tenant,
                cogkos_core::models::ConsolidationStage::FastTrack,
                500,
            )
            .await
        {
            Ok(c) => c,
            Err(e) => {
                debug!(tenant = %tenant, error = %e, "Failed to fetch claims for paradigm shift check");
                continue;
            }
        };

        if claims.is_empty() {
            debug!(tenant = %tenant, "No claims to check for paradigm shift");
            continue;
        }

        // Collect prediction errors from claims that have them
        let prediction_errors: Vec<f64> = claims
            .iter()
            .filter_map(|c| c.last_prediction_error)
            .collect();

        // Use AnomalyDetector to evaluate
        let mut detector = AnomalyDetector::new(0.8);
        let result = detector.detect(&claims, &prediction_errors);

        if result.anomaly_score >= 0.8 {
            warn!(
                tenant = %tenant,
                anomaly_score = result.anomaly_score,
                anomaly_count = result.anomalies.len(),
                recommendation = ?result.recommendation,
                "Paradigm shift trigger condition met — anomaly score >= 0.8"
            );
        } else if result.anomaly_score > 0.3 {
            info!(
                tenant = %tenant,
                anomaly_score = result.anomaly_score,
                assessment = ?result.assessment,
                "Elevated anomaly level detected"
            );
        } else {
            debug!(
                tenant = %tenant,
                anomaly_score = result.anomaly_score,
                "Paradigm shift check: normal"
            );
        }

        if result.recommendation == AnomalyRecommendation::InitiateParadigmShift {
            warn!(
                tenant = %tenant,
                "AnomalyDetector recommends paradigm shift — \
                 requires LLM sandbox verification (Phase 3+ implementation)"
            );
        }

        tenants_checked += 1;
    }

    cogkos_core::monitoring::METRICS
        .record_duration("cogkos_scheduler_task_duration_seconds", start.elapsed());
    info!(
        tenants_checked = tenants_checked,
        duration_ms = start.elapsed().as_millis() as u64,
        "Paradigm shift check complete"
    );
    Ok(tenants_checked)
}

/// Run framework health monitoring — stateless snapshot of knowledge system health
///
/// Computes per-tenant health indicators:
/// - insight_generation_rate: recent Insight-stage claims
/// - prediction_accuracy: 1.0 - avg(last_prediction_error)
/// - knowledge_diversity: distinct node_type count
/// - conflict_resolution_rate: resolved / total conflicts proxy
///
/// Logs metrics as structured tracing events. Warns if overall health < 0.5.
pub(crate) async fn run_framework_health_monitoring(
    stores: &Arc<Stores>,
    _config: &SchedulerConfig,
) -> Result<usize> {
    let start = std::time::Instant::now();
    info!("Running framework health monitoring");

    let tenants = match stores.claims.list_tenants().await {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => vec!["default".to_string()],
        Err(e) => {
            warn!("Failed to list tenants for framework health: {}", e);
            vec!["default".to_string()]
        }
    };

    let mut tenants_checked = 0;

    for tenant in &tenants {
        // 1. Insight generation: count recent Insight-stage claims
        let insight_claims = match stores
            .claims
            .list_claims_by_stage(
                tenant,
                cogkos_core::models::ConsolidationStage::Insight,
                100,
            )
            .await
        {
            Ok(c) => c,
            Err(e) => {
                debug!(tenant = %tenant, error = %e, "Failed to fetch insight claims");
                vec![]
            }
        };
        let insight_generation_rate = insight_claims.len();

        // 2. Prediction accuracy from FastTrack claims with prediction_error
        let recent_claims = match stores
            .claims
            .list_claims_by_stage(
                tenant,
                cogkos_core::models::ConsolidationStage::FastTrack,
                500,
            )
            .await
        {
            Ok(c) => c,
            Err(e) => {
                debug!(tenant = %tenant, error = %e, "Failed to fetch recent claims");
                vec![]
            }
        };

        let errors: Vec<f64> = recent_claims
            .iter()
            .filter_map(|c| c.last_prediction_error)
            .collect();
        let prediction_accuracy = if errors.is_empty() {
            1.0 // No data = assume good (cold start)
        } else {
            let avg_error = errors.iter().sum::<f64>() / errors.len() as f64;
            (1.0 - avg_error).max(0.0)
        };

        // 3. Knowledge diversity: distinct node_types across recent claims
        let node_types: HashSet<String> = recent_claims
            .iter()
            .map(|c| format!("{:?}", c.node_type))
            .collect();
        let knowledge_diversity = node_types.len();

        // 4. Conflict resolution rate: unresolved / (unresolved + some heuristic)
        let unresolved = match stores.claims.list_unresolved_conflicts(tenant, 200).await {
            Ok(c) => c.len(),
            Err(e) => {
                debug!(tenant = %tenant, error = %e, "Failed to fetch unresolved conflicts");
                0
            }
        };
        // Use insight_claims as a proxy for resolved conflicts (elevated = resolved)
        let total_conflict_proxy = unresolved + insight_generation_rate;
        let conflict_resolution_rate = if total_conflict_proxy == 0 {
            1.0 // No conflicts = fully healthy
        } else {
            insight_generation_rate as f64 / total_conflict_proxy as f64
        };

        // Composite health score (weighted average)
        let health_score = prediction_accuracy * 0.4
            + conflict_resolution_rate * 0.3
            + (knowledge_diversity as f64 / 7.0).min(1.0) * 0.15 // 7 = max NodeType variants
            + (insight_generation_rate as f64 / 10.0).min(1.0) * 0.15;

        if health_score < 0.5 {
            warn!(
                tenant = %tenant,
                health_score = format!("{:.3}", health_score),
                prediction_accuracy = format!("{:.3}", prediction_accuracy),
                conflict_resolution_rate = format!("{:.3}", conflict_resolution_rate),
                knowledge_diversity = knowledge_diversity,
                insight_generation_rate = insight_generation_rate,
                unresolved_conflicts = unresolved,
                "Framework health below threshold (< 0.5)"
            );
        } else {
            info!(
                tenant = %tenant,
                health_score = format!("{:.3}", health_score),
                prediction_accuracy = format!("{:.3}", prediction_accuracy),
                conflict_resolution_rate = format!("{:.3}", conflict_resolution_rate),
                knowledge_diversity = knowledge_diversity,
                insight_generation_rate = insight_generation_rate,
                "Framework health snapshot"
            );
        }

        tenants_checked += 1;
    }

    cogkos_core::monitoring::METRICS
        .record_duration("cogkos_scheduler_task_duration_seconds", start.elapsed());
    info!(
        tenants_checked = tenants_checked,
        duration_ms = start.elapsed().as_millis() as u64,
        "Framework health monitoring complete"
    );
    Ok(tenants_checked)
}

/// Run collective wisdom four-conditions check for multi-agent knowledge quality
///
/// For each tenant, extracts distinct agent_ids from claim.claimant,
/// builds InsightSource per agent, and runs the four-conditions health check:
/// - Diversity: Shannon entropy of agent source distribution
/// - Independence: provenance-based source independence
/// - Decentralization: Gini coefficient of agent influence
/// - Aggregation effectiveness: aggregated vs best single source
///
/// Logs structured metrics per tenant. Warns if any condition is unhealthy.
pub(crate) async fn run_collective_wisdom_check(
    stores: &Arc<Stores>,
    _config: &SchedulerConfig,
) -> Result<usize> {
    use cogkos_federation::health::{
        InsightSource, Prediction, ProvenanceInfo, calculate_collective_health,
    };
    use std::collections::HashMap;

    let start = std::time::Instant::now();
    info!("Running collective wisdom four-conditions check");

    let tenants = match stores.claims.list_tenants().await {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => vec!["default".to_string()],
        Err(e) => {
            warn!("Failed to list tenants for collective wisdom check: {}", e);
            vec!["default".to_string()]
        }
    };

    let mut tenants_checked = 0;

    for tenant in &tenants {
        // Fetch recent claims (FastTrack = actively used knowledge)
        let claims = match stores
            .claims
            .list_claims_by_stage(
                tenant,
                cogkos_core::models::ConsolidationStage::FastTrack,
                500,
            )
            .await
        {
            Ok(c) => c,
            Err(e) => {
                debug!(tenant = %tenant, error = %e, "Failed to fetch claims for collective wisdom check");
                continue;
            }
        };

        if claims.len() < 2 {
            debug!(tenant = %tenant, claims = claims.len(), "Too few claims for collective wisdom check");
            continue;
        }

        // Group claims by agent_id (extracted from claimant field)
        let mut agent_claims: HashMap<String, Vec<&cogkos_core::models::EpistemicClaim>> =
            HashMap::new();
        for claim in &claims {
            let agent_id = extract_agent_id(&claim.claimant);
            agent_claims.entry(agent_id).or_default().push(claim);
        }

        if agent_claims.len() < 2 {
            debug!(
                tenant = %tenant,
                agents = agent_claims.len(),
                "Single agent — collective wisdom check requires >= 2 agents"
            );
            continue;
        }

        // Build InsightSource per agent
        let insight_sources: Vec<InsightSource> = agent_claims
            .iter()
            .map(|(agent_id, agent_specific_claims)| {
                let avg_confidence = agent_specific_claims
                    .iter()
                    .map(|c| c.confidence)
                    .sum::<f64>()
                    / agent_specific_claims.len() as f64;

                let influence = agent_specific_claims.len() as f64 / claims.len() as f64;

                InsightSource {
                    source_id: agent_id.clone(),
                    provenance: ProvenanceInfo {
                        source_id: agent_id.clone(),
                        source_type: "agent".to_string(),
                        upstream_sources: vec![],
                    },
                    influence,
                    confidence: avg_confidence,
                    predictions: agent_specific_claims
                        .iter()
                        .take(5)
                        .map(|c| Prediction {
                            content: c.content.chars().take(100).collect(),
                            confidence: c.confidence,
                        })
                        .collect(),
                }
            })
            .collect();

        let health = calculate_collective_health(&insight_sources);

        // Log results
        if !health.warnings.is_empty() {
            warn!(
                tenant = %tenant,
                agent_count = agent_claims.len(),
                claim_count = claims.len(),
                overall_health = format!("{:.3}", health.overall_health),
                diversity = format!("{:.3}", health.diversity_score),
                independence = format!("{:.3}", health.independence_score),
                decentralization = format!("{:.3}", health.decentralization_score),
                aggregation = format!("{:.3}", health.aggregation_effectiveness),
                warnings = ?health.warnings,
                "Collective wisdom: conditions not fully met"
            );
        } else {
            info!(
                tenant = %tenant,
                agent_count = agent_claims.len(),
                claim_count = claims.len(),
                overall_health = format!("{:.3}", health.overall_health),
                diversity = format!("{:.3}", health.diversity_score),
                independence = format!("{:.3}", health.independence_score),
                decentralization = format!("{:.3}", health.decentralization_score),
                aggregation = format!("{:.3}", health.aggregation_effectiveness),
                "Collective wisdom: all four conditions healthy"
            );
        }

        tenants_checked += 1;
    }

    cogkos_core::monitoring::METRICS
        .record_duration("cogkos_scheduler_task_duration_seconds", start.elapsed());
    info!(
        tenants_checked = tenants_checked,
        duration_ms = start.elapsed().as_millis() as u64,
        "Collective wisdom check complete"
    );
    Ok(tenants_checked)
}

/// Extract agent_id from a Claimant enum.
///
/// Maps each variant to a unique identifier string used for collective wisdom grouping.
pub(crate) fn extract_agent_id(claimant: &cogkos_core::models::Claimant) -> String {
    use cogkos_core::models::Claimant;
    match claimant {
        Claimant::Agent { agent_id, .. } => agent_id.clone(),
        Claimant::Human { user_id, .. } => format!("human:{}", user_id),
        Claimant::System => "system".to_string(),
        Claimant::ExternalPublic { source_name } => format!("external:{}", source_name),
    }
}
