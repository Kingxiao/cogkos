//! System self-reflection MCP tools — health monitoring, evolution status, introspection

use std::collections::HashSet;
use cogkos_core::evolution::paradigm_shift::anomaly::{AnomalyDetector, AnomalyRecommendation};
use cogkos_store::Stores;

/// Compute framework health snapshot for a tenant (reuses tasks_phase3 logic)
pub async fn handle_system_health(
    tenant_id: &str,
    stores: Stores,
) -> Result<serde_json::Value, cogkos_core::CogKosError> {
    // 1. Insight generation rate
    let insight_claims = stores
        .claims
        .list_claims_by_stage(
            tenant_id,
            cogkos_core::models::ConsolidationStage::Insight,
            100,
        )
        .await
        .unwrap_or_default();
    let insight_generation_rate = insight_claims.len();

    // 2. Prediction accuracy
    let recent_claims = stores
        .claims
        .list_claims_by_stage(
            tenant_id,
            cogkos_core::models::ConsolidationStage::FastTrack,
            500,
        )
        .await
        .unwrap_or_default();

    let errors: Vec<f64> = recent_claims
        .iter()
        .filter_map(|c| c.last_prediction_error)
        .collect();
    let prediction_accuracy = if errors.is_empty() {
        1.0
    } else {
        let avg_error = errors.iter().sum::<f64>() / errors.len() as f64;
        (1.0 - avg_error).max(0.0)
    };

    // 3. Knowledge diversity
    let node_types: HashSet<String> = recent_claims
        .iter()
        .map(|c| format!("{:?}", c.node_type))
        .collect();
    let knowledge_diversity = node_types.len();

    // 4. Conflict resolution rate
    let unresolved = stores
        .claims
        .list_unresolved_conflicts(tenant_id, 200)
        .await
        .map(|c| c.len())
        .unwrap_or(0);
    let total_conflict_proxy = unresolved + insight_generation_rate;
    let conflict_resolution_rate = if total_conflict_proxy == 0 {
        1.0
    } else {
        insight_generation_rate as f64 / total_conflict_proxy as f64
    };

    // Composite health score
    let health_score = prediction_accuracy * 0.4
        + conflict_resolution_rate * 0.3
        + (knowledge_diversity as f64 / 7.0).min(1.0) * 0.15
        + (insight_generation_rate as f64 / 10.0).min(1.0) * 0.15;

    Ok(serde_json::json!({
        "tenant_id": tenant_id,
        "health_score": (health_score * 1000.0).round() / 1000.0,
        "indicators": {
            "prediction_accuracy": (prediction_accuracy * 1000.0).round() / 1000.0,
            "conflict_resolution_rate": (conflict_resolution_rate * 1000.0).round() / 1000.0,
            "knowledge_diversity": knowledge_diversity,
            "insight_generation_rate": insight_generation_rate,
        },
        "alerts": {
            "health_warning": health_score < 0.5,
            "unresolved_conflicts": unresolved,
        },
        "total_claims_analyzed": recent_claims.len(),
    }))
}

/// Compute evolution/paradigm-shift status for a tenant
pub async fn handle_evolution_status(
    tenant_id: &str,
    stores: Stores,
) -> Result<serde_json::Value, cogkos_core::CogKosError> {
    let claims = stores
        .claims
        .list_claims_by_stage(
            tenant_id,
            cogkos_core::models::ConsolidationStage::FastTrack,
            500,
        )
        .await
        .unwrap_or_default();

    if claims.is_empty() {
        return Ok(serde_json::json!({
            "tenant_id": tenant_id,
            "anomaly_score": 0.0,
            "assessment": "NoData",
            "recommendation": "Continue",
            "anomalies": [],
            "claims_analyzed": 0,
        }));
    }

    let prediction_errors: Vec<f64> = claims
        .iter()
        .filter_map(|c| c.last_prediction_error)
        .collect();

    let mut detector = AnomalyDetector::new(0.8);
    let result = detector.detect(&claims, &prediction_errors);

    let anomaly_details: Vec<serde_json::Value> = result
        .anomalies
        .iter()
        .map(|a| {
            serde_json::json!({
                "anomaly_type": format!("{:?}", a.anomaly_type),
                "confidence": a.confidence,
                "description": a.description,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "tenant_id": tenant_id,
        "anomaly_score": (result.anomaly_score * 1000.0).round() / 1000.0,
        "assessment": format!("{:?}", result.assessment),
        "recommendation": format!("{:?}", result.recommendation),
        "paradigm_shift_triggered": result.recommendation == AnomalyRecommendation::InitiateParadigmShift,
        "anomalies": anomaly_details,
        "claims_analyzed": claims.len(),
        "prediction_errors_tracked": prediction_errors.len(),
    }))
}

/// System introspection — multi-tenant health overview
pub async fn handle_system_introspection(
    stores: Stores,
) -> Result<serde_json::Value, cogkos_core::CogKosError> {
    let tenants = stores
        .claims
        .list_tenants()
        .await
        .unwrap_or_else(|_| vec!["default".to_string()]);

    let mut tenant_health = Vec::new();
    for tenant in &tenants {
        let health = handle_system_health(tenant, stores.clone()).await?;
        tenant_health.push(health);
    }

    // Aggregate statistics
    let total_tenants = tenants.len();
    let healthy_count = tenant_health
        .iter()
        .filter(|h| {
            h.get("health_score")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
                >= 0.5
        })
        .count();

    Ok(serde_json::json!({
        "total_tenants": total_tenants,
        "healthy_tenants": healthy_count,
        "unhealthy_tenants": total_tenants - healthy_count,
        "tenants": tenant_health,
    }))
}
