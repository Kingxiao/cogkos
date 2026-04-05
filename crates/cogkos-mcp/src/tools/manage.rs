//! Manage claim and batch invalidate handlers

use cogkos_core::authority::AuthorityTier;
use cogkos_core::models::*;
use cogkos_core::{CogKosError, Result};
use cogkos_store::{BatchInvalidateFilter, ClaimStore};

use super::types::*;

/// Handle manage_claim: promote, demote, set confidence, or retract
pub async fn handle_manage_claim(
    req: ManageClaimRequest,
    tenant_id: &str,
    claim_store: &dyn ClaimStore,
) -> Result<serde_json::Value> {
    let claim_id = uuid::Uuid::parse_str(&req.claim_id)
        .map_err(|e| CogKosError::InvalidInput(e.to_string()))?;

    let mut claim = claim_store.get_claim(claim_id, tenant_id).await?;

    match req.action {
        ManageAction::Promote { knowledge_type } => {
            claim.knowledge_type = match knowledge_type.as_str() {
                "Business" => KnowledgeType::Business,
                "Experiential" => KnowledgeType::Experiential,
                _ => {
                    return Err(CogKosError::InvalidInput(
                        "Invalid knowledge_type for promote".into(),
                    ));
                }
            };
            let tier = AuthorityTier::resolve(&claim);
            claim.durability = tier.recommended_durability();
        }
        ManageAction::Demote { knowledge_type } => {
            claim.knowledge_type = match knowledge_type.as_str() {
                "Experiential" => KnowledgeType::Experiential,
                "Business" => KnowledgeType::Business,
                _ => {
                    return Err(CogKosError::InvalidInput(
                        "Invalid knowledge_type for demote".into(),
                    ));
                }
            };
            let tier = AuthorityTier::resolve(&claim);
            claim.durability = tier.recommended_durability();
        }
        ManageAction::SetConfidence { confidence } => {
            claim.confidence = confidence.clamp(0.0, 1.0);
        }
        ManageAction::Retract { reason } => {
            claim.epistemic_status = EpistemicStatus::Retracted;
            claim.confidence = 0.0;
            if let Some(r) = reason {
                claim.metadata.insert(
                    "retraction_reason".to_string(),
                    serde_json::Value::String(r),
                );
            }
        }
    }

    claim.updated_at = chrono::Utc::now();
    claim_store.update_claim(&claim).await?;

    Ok(serde_json::json!({
        "status": "updated",
        "claim_id": claim_id.to_string(),
        "new_knowledge_type": format!("{}", claim.knowledge_type),
        "new_confidence": claim.confidence,
        "new_epistemic_status": claim.epistemic_status.as_db_str(),
    }))
}

/// Handle batch_invalidate: retract claims matching filter criteria
pub async fn handle_batch_invalidate(
    req: BatchInvalidateRequest,
    tenant_id: &str,
    claim_store: &dyn ClaimStore,
) -> Result<serde_json::Value> {
    // Require at least one filter to prevent accidental mass invalidation
    if req.domain.is_none()
        && req.tags.is_none()
        && req.created_before.is_none()
        && req.knowledge_type.is_none()
    {
        return Err(CogKosError::InvalidInput(
            "At least one filter (domain, tags, created_before, knowledge_type) is required".into(),
        ));
    }

    let filter = BatchInvalidateFilter {
        domain: req.domain,
        tags: req.tags,
        created_before: req.created_before,
        knowledge_type: req.knowledge_type,
    };

    let affected = claim_store.batch_invalidate(tenant_id, filter).await?;

    Ok(serde_json::json!({
        "status": "completed",
        "affected_claims": affected,
    }))
}
