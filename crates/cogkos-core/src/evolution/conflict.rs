use crate::models::{
    ConflictRecord, ConflictType, EpistemicClaim, EpistemicStatus, ResolutionStatus,
};

/// Detect conflicts between two claims
/// Returns Some(ConflictRecord) if a conflict is detected
pub fn detect_conflict(
    claim_a: &EpistemicClaim,
    claim_b: &EpistemicClaim,
) -> Option<ConflictRecord> {
    // Don't compare with self
    if claim_a.id == claim_b.id {
        return None;
    }

    // Check if both claims are valid (not retracted/superseded)
    if !is_valid(claim_a) || !is_valid(claim_b) {
        return None;
    }

    // Simple string-based conflict detection (Phase 1)
    // Phase 3+: Use LLM for semantic conflict detection
    let conflict_type = detect_conflict_type(claim_a, claim_b);

    conflict_type.map(|ct| {
        let mut record = ConflictRecord::new(&claim_a.tenant_id, claim_a.id, claim_b.id, ct);
        record.resolution_status = ResolutionStatus::Open;
        record
    })
}

/// Check if claim is valid (not retracted/superseded)
fn is_valid(claim: &EpistemicClaim) -> bool {
    !matches!(
        claim.epistemic_status,
        EpistemicStatus::Retracted | EpistemicStatus::Superseded
    )
}

/// Detect the type of conflict between two claims
fn detect_conflict_type(
    claim_a: &EpistemicClaim,
    claim_b: &EpistemicClaim,
) -> Option<ConflictType> {
    // Check for direct contradiction indicators
    let a_content = claim_a.content.to_lowercase();
    let b_content = claim_b.content.to_lowercase();

    // Simple heuristic: negation detection
    let negation_words = ["not ", "no ", "false", "incorrect", "wrong", "never"];

    let a_has_negation = negation_words.iter().any(|w| a_content.contains(w));
    let b_has_negation = negation_words.iter().any(|w| b_content.contains(w));

    // If one is negation and other isn't, and content is similar
    if a_has_negation != b_has_negation {
        // Check content similarity (simple word overlap)
        let a_words: std::collections::HashSet<_> = a_content.split_whitespace().collect();
        let b_words: std::collections::HashSet<_> = b_content.split_whitespace().collect();

        let common_words: std::collections::HashSet<_> = a_words.intersection(&b_words).collect();
        let total_unique_words: std::collections::HashSet<_> = a_words.union(&b_words).collect();

        if !total_unique_words.is_empty() {
            let similarity = common_words.len() as f64 / total_unique_words.len() as f64;
            if similarity > 0.5 {
                return Some(ConflictType::DirectContradiction);
            }
        }
    }

    // Check for temporal shift (same entity, different time periods)
    if claim_a.node_type == claim_b.node_type
        && !claim_a.derived_from.is_empty()
        && !claim_b.derived_from.is_empty()
    {
        // If they share derivation but have different validity periods
        if claim_a
            .derived_from
            .iter()
            .any(|id| claim_b.derived_from.contains(id))
        {
            return Some(ConflictType::TemporalShift);
        }
    }

    // Check for source disagreement (different sources, same claim type)
    if claim_a.provenance.source_id != claim_b.provenance.source_id
        && claim_a.node_type == claim_b.node_type
    {
        // Simple semantic similarity check
        let similarity = calculate_content_similarity(&a_content, &b_content);
        if similarity > 0.7 && similarity < 1.0 {
            return Some(ConflictType::SourceDisagreement);
        }
    }

    // Context-dependent conflicts (different scopes/conditions)
    // Detected when content is similar but not identical, and both have different scopes
    if claim_a.tenant_id != claim_b.tenant_id {
        let similarity = calculate_content_similarity(&a_content, &b_content);
        if similarity > 0.6 {
            return Some(ConflictType::ContextualDifference);
        }
    }

    None
}

/// Calculate simple content similarity (Jaccard index on words)
fn calculate_content_similarity(a: &str, b: &str) -> f64 {
    let a_words: std::collections::HashSet<_> = a.split_whitespace().collect();
    let b_words: std::collections::HashSet<_> = b.split_whitespace().collect();

    if a_words.is_empty() || b_words.is_empty() {
        return 0.0;
    }

    let intersection: std::collections::HashSet<_> = a_words.intersection(&b_words).collect();
    let union: std::collections::HashSet<_> = a_words.union(&b_words).collect();

    intersection.len() as f64 / union.len() as f64
}

/// Batch conflict detection for a new claim against existing claims
pub fn detect_conflicts_batch(
    new_claim: &EpistemicClaim,
    existing_claims: &[EpistemicClaim],
) -> Vec<ConflictRecord> {
    existing_claims
        .iter()
        .filter_map(|existing| detect_conflict(new_claim, existing))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    fn create_test_claim(content: &str, claimant: Claimant, source_id: &str) -> EpistemicClaim {
        let prov = ProvenanceRecord::new(
            source_id.to_string(),
            "test".to_string(),
            "test".to_string(),
        );
        EpistemicClaim::new(
            content.to_string(),       // content first
            "test-tenant".to_string(), // tenant_id second
            NodeType::Entity,
            claimant,
            AccessEnvelope::new("test-tenant"),
            prov,
        )
    }

    #[test]
    fn test_no_self_conflict() {
        let claim = create_test_claim("test", Claimant::System, "source1");
        assert!(detect_conflict(&claim, &claim).is_none());
    }

    #[test]
    fn test_direct_contradiction() {
        // Use incorrect/true which are both in negation words list
        let claim_a = create_test_claim("The statement is incorrect", Claimant::System, "source1");
        let claim_b = create_test_claim("The statement is true", Claimant::System, "source2");

        let conflict = detect_conflict(&claim_a, &claim_b);
        assert!(conflict.is_some());
        assert_eq!(
            conflict.unwrap().conflict_type,
            ConflictType::DirectContradiction
        );
    }

    #[test]
    fn test_source_disagreement() {
        // Use very similar content to pass similarity threshold > 0.7
        let claim_a = create_test_claim("The price is 100", Claimant::System, "source1");
        let claim_b = create_test_claim("The price is 100 today", Claimant::System, "source2");

        let conflict = detect_conflict(&claim_a, &claim_b);
        assert!(conflict.is_some());
        assert_eq!(
            conflict.unwrap().conflict_type,
            ConflictType::SourceDisagreement
        );
    }

    #[test]
    fn test_no_conflict_different_topics() {
        let claim_a = create_test_claim("The sky is blue", Claimant::System, "source1");
        let claim_b = create_test_claim("Pizza is delicious", Claimant::System, "source2");

        let conflict = detect_conflict(&claim_a, &claim_b);
        assert!(conflict.is_none());
    }

    #[test]
    fn test_content_similarity() {
        assert!(calculate_content_similarity("hello world", "hello world") > 0.99);
        assert!(calculate_content_similarity("hello world", "hello there") > 0.3);
        assert!(calculate_content_similarity("hello world", "foo bar") < 0.1);
    }
}
