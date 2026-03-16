//! Conflict detection task
//!
//! Detects conflicts between newly written claims and existing knowledge.

use cogkos_core::Result;
use cogkos_core::evolution::conflict::detect_conflicts_batch;
use cogkos_core::models::{ConflictRecord, ConflictType, EpistemicClaim, ResolutionStatus};
use cogkos_store::Stores;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Configuration for conflict detection
#[derive(Debug, Clone)]
pub struct ConflictDetectionConfig {
    /// Maximum number of existing claims to compare against
    pub max_comparison_batch: usize,
    /// Similarity threshold for conflict detection
    pub similarity_threshold: f64,
    /// Whether to use semantic similarity (requires LLM in Phase 3)
    pub use_semantic: bool,
}

impl Default for ConflictDetectionConfig {
    fn default() -> Self {
        Self {
            max_comparison_batch: 100,
            similarity_threshold: 0.6,
            use_semantic: false,
        }
    }
}

/// Run conflict detection for a newly written claim
///
/// This is event-driven - called after each write to the knowledge base
#[tracing::instrument(skip(stores, new_claim, config), fields(claim_id = %new_claim.id, tenant = %new_claim.tenant_id))]
pub async fn detect_conflicts(
    stores: &Arc<Stores>,
    new_claim: &EpistemicClaim,
    config: &ConflictDetectionConfig,
) -> Result<Vec<ConflictRecord>> {
    info!(
        claim_id = %new_claim.id,
        tenant_id = %new_claim.tenant_id,
        "Running conflict detection for new claim"
    );

    // Get existing claims to compare against
    // For efficiency, we limit to recent/modified claims
    let existing_claims = stores
        .claims
        .search_claims(
            &new_claim.tenant_id,
            &new_claim.content,
            config.max_comparison_batch,
        )
        .await?;

    debug!(
        claim_id = %new_claim.id,
        compared_against = existing_claims.len(),
        "Retrieved existing claims for comparison"
    );

    // Detect conflicts
    let conflicts = detect_conflicts_batch(new_claim, &existing_claims);

    if conflicts.is_empty() {
        debug!(claim_id = %new_claim.id, "No conflicts detected");
        return Ok(vec![]);
    }

    info!(
        claim_id = %new_claim.id,
        conflict_count = conflicts.len(),
        "Detected conflicts"
    );

    // Store conflicts in the database
    for mut conflict in conflicts.iter().cloned() {
        // Add description based on conflict type
        conflict.description = Some(describe_conflict(&conflict.conflict_type));

        if let Err(e) = stores.claims.insert_conflict(&conflict).await {
            error!(error = %e, conflict_id = %conflict.id, "Failed to store conflict record");
        }
    }

    Ok(conflicts)
}

/// Detect conflicts for all claims in a tenant (periodic task)
pub async fn detect_conflicts_periodic(
    stores: &Arc<Stores>,
    tenant_id: &str,
    limit: usize,
) -> Result<Vec<ConflictRecord>> {
    info!(tenant_id = %tenant_id, "Running periodic conflict detection");

    // Get FastTrack claims (newest assertions)
    let claims = stores
        .claims
        .list_claims_by_stage(
            tenant_id,
            cogkos_core::models::ConsolidationStage::FastTrack,
            limit,
        )
        .await?;

    let mut all_conflicts = Vec::new();

    for claim in claims {
        // Get claims to compare (exclude the current one)
        let existing = stores
            .claims
            .search_claims(tenant_id, &claim.content, 20)
            .await?;

        let conflicts = detect_conflicts_batch(&claim, &existing);

        for mut conflict in conflicts {
            conflict.description = Some(describe_conflict(&conflict.conflict_type));

            if let Err(e) = stores.claims.insert_conflict(&conflict).await {
                warn!(error = %e, "Failed to store conflict");
            } else {
                all_conflicts.push(conflict);
            }
        }
    }

    info!(
        tenant_id = %tenant_id,
        total_conflicts = all_conflicts.len(),
        "Periodic conflict detection complete"
    );

    Ok(all_conflicts)
}

/// Calculate conflict severity based on claim properties
pub fn calculate_conflict_severity(claim_a: &EpistemicClaim, claim_b: &EpistemicClaim) -> f64 {
    // Base severity from confidence difference
    let confidence_diff = (claim_a.confidence - claim_b.confidence).abs();

    // Higher confidence claims in conflict = more severe
    let avg_confidence = (claim_a.confidence + claim_b.confidence) / 2.0;

    // Source independence increases severity (different sources = more important)
    let source_independence = if claim_a.provenance.source_id != claim_b.provenance.source_id {
        1.0
    } else {
        0.5
    };

    // Combine factors
    let severity = (1.0 - confidence_diff) * avg_confidence * source_independence;
    severity.clamp(0.0, 1.0)
}

/// Resolve a detected conflict
#[tracing::instrument(skip(stores))]
pub async fn resolve_conflict(
    stores: &Arc<Stores>,
    conflict_id: uuid::Uuid,
    resolution: ResolutionStatus,
    note: Option<String>,
) -> Result<()> {
    info!(conflict_id = %conflict_id, resolution = ?resolution, "Resolving conflict");

    stores
        .claims
        .resolve_conflict(conflict_id, resolution, note)
        .await?;

    info!(conflict_id = %conflict_id, "Conflict resolved");
    Ok(())
}

/// Generate human-readable description of conflict type
fn describe_conflict(conflict_type: &ConflictType) -> String {
    match conflict_type {
        ConflictType::DirectContradiction => "Direct contradiction between two claims".to_string(),
        ConflictType::ContextDependent => "Conflict depends on context or scope".to_string(),
        ConflictType::TemporalShift => {
            "Temporal inconsistency - same entity, different time periods".to_string()
        }
        ConflictType::TemporalInconsistency => {
            "Temporal inconsistency in validity periods".to_string()
        }
        ConflictType::SourceDisagreement => {
            "Different sources provide conflicting information".to_string()
        }
        ConflictType::ConfidenceMismatch => {
            "Significant confidence discrepancy between similar claims".to_string()
        }
        ConflictType::ContextualDifference => {
            "Claims valid in different contexts but conflict".to_string()
        }
    }
}

/// Detect conflicts using LLM semantic analysis
///
/// Sends both claims to an LLM to determine if they semantically conflict,
/// catching subtleties that rule-based detection misses.
pub async fn detect_llm_semantic_conflict(
    claim_a: &EpistemicClaim,
    claim_b: &EpistemicClaim,
    llm_client: &dyn cogkos_llm::LlmClient,
) -> Option<ConflictType> {
    use cogkos_llm::types::{LlmRequest, Message, Role};

    let system_prompt = "You are a conflict detection system. Given two knowledge claims, determine if they contradict each other.\n\
        Respond with EXACTLY one of these labels on the first line:\n\
        - NONE — no conflict\n\
        - DIRECT_CONTRADICTION — the claims directly contradict\n\
        - TEMPORAL_SHIFT — the claims differ due to time\n\
        - CONTEXT_DEPENDENT — the claims address different scopes but appear conflicting\n\
        - CONFIDENCE_MISMATCH — same topic, very different confidence levels\n\
        - SOURCE_DISAGREEMENT — same topic from incompatible sources\n\
        - CONTEXTUAL_DIFFERENCE — meaning has shifted subtly\n\n\
        On the second line, provide a brief reason (one sentence).";

    let user_prompt = format!(
        "Claim A (confidence {:.2}): {}\n\nClaim B (confidence {:.2}): {}",
        claim_a.confidence, claim_a.content, claim_b.confidence, claim_b.content
    );

    let request = LlmRequest {
        messages: vec![
            Message {
                role: Role::System,
                content: system_prompt.to_string(),
            },
            Message {
                role: Role::User,
                content: user_prompt,
            },
        ],
        temperature: 0.0,
        max_tokens: Some(100),
        ..Default::default()
    };

    match llm_client.chat(request).await {
        Ok(response) => {
            let first_line = response.content.lines().next().unwrap_or("").trim();
            match first_line {
                "DIRECT_CONTRADICTION" => Some(ConflictType::DirectContradiction),
                "TEMPORAL_SHIFT" => Some(ConflictType::TemporalShift),
                "CONTEXT_DEPENDENT" => Some(ConflictType::ContextDependent),
                "CONFIDENCE_MISMATCH" => Some(ConflictType::ConfidenceMismatch),
                "SOURCE_DISAGREEMENT" => Some(ConflictType::SourceDisagreement),
                "CONTEXTUAL_DIFFERENCE" => Some(ConflictType::ContextualDifference),
                "TEMPORAL_INCONSISTENCY" => Some(ConflictType::TemporalInconsistency),
                _ => None,
            }
        }
        Err(e) => {
            warn!("LLM semantic conflict detection failed: {}", e);
            None
        }
    }
}

/// Enhanced conflict detection that combines rule-based and LLM-based analysis
pub async fn detect_conflicts_enhanced(
    stores: &Arc<Stores>,
    new_claim: &EpistemicClaim,
    config: &ConflictDetectionConfig,
    llm_client: Option<&dyn cogkos_llm::LlmClient>,
) -> Result<Vec<ConflictRecord>> {
    // First run rule-based detection
    let mut conflicts = detect_conflicts(stores, new_claim, config).await?;

    // If LLM semantic detection is enabled and we have a client
    if config.use_semantic
        && let Some(client) = llm_client {
            let existing_claims = stores
                .claims
                .search_claims(
                    &new_claim.tenant_id,
                    &new_claim.content,
                    config.max_comparison_batch,
                )
                .await?;

            // Only run LLM on pairs not already flagged by rule-based detection
            let flagged_ids: std::collections::HashSet<_> = conflicts
                .iter()
                .flat_map(|c| vec![c.claim_a_id, c.claim_b_id])
                .collect();

            for existing in &existing_claims {
                if existing.id == new_claim.id || flagged_ids.contains(&existing.id) {
                    continue;
                }

                if let Some(conflict_type) =
                    detect_llm_semantic_conflict(new_claim, existing, client).await
                {
                    let mut record = ConflictRecord::new(
                        new_claim.tenant_id.clone(),
                        new_claim.id,
                        existing.id,
                        conflict_type,
                    );
                    record.description = Some(describe_conflict(&conflict_type));
                    record.severity = calculate_conflict_severity(new_claim, existing);

                    if let Err(e) = stores.claims.insert_conflict(&record).await {
                        error!(error = %e, "Failed to store LLM-detected conflict");
                    } else {
                        conflicts.push(record);
                    }
                }
            }
        }

    Ok(conflicts)
}

/// Get open conflicts for a tenant
pub async fn get_open_conflicts(
    stores: &Arc<Stores>,
    tenant_id: &str,
) -> Result<Vec<ConflictRecord>> {
    // Query claims and filter for those with conflicts
    // This is a simplified implementation
    let claims = stores
        .claims
        .list_claims_by_stage(
            tenant_id,
            cogkos_core::models::ConsolidationStage::FastTrack,
            100,
        )
        .await?;

    let mut conflicts = Vec::new();
    for claim in claims {
        let claim_conflicts = stores.claims.get_conflicts_for_claim(claim.id).await?;
        for conflict in claim_conflicts {
            if conflict.resolution_status == ResolutionStatus::Open {
                conflicts.push(conflict);
            }
        }
    }

    Ok(conflicts)
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
    fn test_conflict_severity_calculation() {
        let claim_a = create_test_claim("Product is good", 0.9, "source1");
        let claim_b = create_test_claim("Product is not good", 0.8, "source2");

        let severity = calculate_conflict_severity(&claim_a, &claim_b);

        assert!(severity > 0.0);
        assert!(severity <= 1.0);
    }

    #[test]
    fn test_describe_conflict() {
        let desc = describe_conflict(&ConflictType::DirectContradiction);
        assert!(desc.contains("contradiction"));
    }
}
