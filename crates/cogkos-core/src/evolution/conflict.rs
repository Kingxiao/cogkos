use crate::authority::AuthorityTier;
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
        // Enrich description with content summaries for human readability
        let summary_a: String = claim_a.content.chars().take(80).collect();
        let summary_b: String = claim_b.content.chars().take(80).collect();
        record.description = Some(format!(
            "{:?}: [{}...] vs [{}...]",
            ct, summary_a, summary_b
        ));
        record.resolution_status = ResolutionStatus::Open;

        // Add authority tier suggestion when tiers differ
        let tier_a = AuthorityTier::resolve(claim_a);
        let tier_b = AuthorityTier::resolve(claim_b);
        if tier_a != tier_b {
            let (preferred_id, preferred_tier, other_tier) = if tier_a > tier_b {
                (claim_a.id, tier_a, tier_b)
            } else {
                (claim_b.id, tier_b, tier_a)
            };
            record.resolution = Some(serde_json::json!({
                "authority_suggestion": {
                    "preferred_claim_id": preferred_id.to_string(),
                    "preferred_tier": preferred_tier.as_str(),
                    "other_tier": other_tier.as_str(),
                    "reason": format!(
                        "Claim {} has higher authority ({}) vs ({})",
                        preferred_id, preferred_tier, other_tier
                    ),
                }
            }));
        }

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
    let a_content = claim_a.content.to_lowercase();
    let b_content = claim_b.content.to_lowercase();

    // 1. Negation-based contradiction (EN + CN)
    let negation_words = [
        "not ",
        "no ",
        "false",
        "incorrect",
        "wrong",
        "never", // EN
        "不",
        "没有",
        "并非",
        "未",
        "无法",
        "低于",
        "少于",
        "远低",
        "远慢", // CN
    ];
    let a_negation = negation_words.iter().any(|w| a_content.contains(w));
    let b_negation = negation_words.iter().any(|w| b_content.contains(w));

    let similarity = calculate_content_similarity(&a_content, &b_content);

    if a_negation != b_negation && similarity > 0.3 {
        return Some(ConflictType::DirectContradiction);
    }

    // 2. Numeric contradiction — same topic, different numbers
    // Require high topic similarity to avoid false positives from incidental numbers
    if similarity > 0.5 && detect_numeric_contradiction(&a_content, &b_content) {
        return Some(ConflictType::ConfidenceMismatch);
    }

    // 3. Confidence gap — same topic, confidence diff > 0.3
    if similarity > 0.4
        && (claim_a.confidence - claim_b.confidence).abs() > 0.3
        && claim_a.provenance.source_id != claim_b.provenance.source_id
    {
        return Some(ConflictType::ConfidenceMismatch);
    }

    // 4. Temporal shift (same entity, shared derivation)
    if claim_a.node_type == claim_b.node_type
        && !claim_a.derived_from.is_empty()
        && claim_a
            .derived_from
            .iter()
            .any(|id| claim_b.derived_from.contains(id))
    {
        return Some(ConflictType::TemporalShift);
    }

    // 5. Temporal inconsistency (similar content but valid_start times differ significantly)
    if similarity > 0.4 {
        let time_diff = (claim_a.t_valid_start - claim_b.t_valid_start)
            .num_hours()
            .unsigned_abs();
        // >30 days apart on similar topic = temporal inconsistency
        if time_diff > 720 {
            return Some(ConflictType::TemporalInconsistency);
        }
    }

    // 6. Context dependent (same content, different node_type = meaning varies by context)
    if claim_a.node_type != claim_b.node_type && similarity > 0.6 {
        return Some(ConflictType::ContextDependent);
    }

    // 7. Source disagreement (different sources, high similarity but not identical)
    if claim_a.provenance.source_id != claim_b.provenance.source_id
        && claim_a.node_type == claim_b.node_type
        && similarity > 0.5
        && similarity < 1.0
    {
        return Some(ConflictType::SourceDisagreement);
    }

    // 8. Contextual difference (different tenants, similar content)
    if claim_a.tenant_id != claim_b.tenant_id && similarity > 0.6 {
        return Some(ConflictType::ContextualDifference);
    }

    None
}

/// Detect numeric contradiction: both texts contain numbers but they differ significantly
fn detect_numeric_contradiction(a: &str, b: &str) -> bool {
    // Match numbers with optional % or 万/亿 suffix
    let re = match regex::Regex::new(r"(\d+(?:\.\d+)?)\s*[%万亿]?") {
        Ok(r) => r,
        Err(_) => return false,
    };
    let extract = |text: &str| -> Vec<f64> {
        re.captures_iter(text)
            .filter_map(|c| c.get(1).and_then(|m| m.as_str().parse::<f64>().ok()))
            .filter(|n| *n > 0.0) // skip zero
            .collect()
    };
    let a_nums = extract(a);
    let b_nums = extract(b);
    if a_nums.is_empty() || b_nums.is_empty() {
        return false;
    }
    // If any pair of numbers differs by >50% relative, it's a contradiction
    for an in &a_nums {
        for bn in &b_nums {
            let diff = (an - bn).abs();
            let max = an.abs().max(bn.abs());
            if max > 0.0 && diff / max > 0.5 {
                return true;
            }
        }
    }
    false
}

/// Content similarity — uses char-level bigrams for CJK, word-level for Latin
fn calculate_content_similarity(a: &str, b: &str) -> f64 {
    let is_cjk = a.chars().any(|c| c > '\u{2E80}') || b.chars().any(|c| c > '\u{2E80}');

    if is_cjk {
        // Char bigram Jaccard for Chinese/Japanese/Korean
        let bigrams = |s: &str| -> std::collections::HashSet<String> {
            let chars: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
            if chars.len() < 2 {
                return chars.iter().map(|c| c.to_string()).collect();
            }
            chars
                .windows(2)
                .map(|w| w.iter().collect::<String>())
                .collect()
        };
        let a_bi = bigrams(a);
        let b_bi = bigrams(b);
        if a_bi.is_empty() || b_bi.is_empty() {
            return 0.0;
        }
        let intersection = a_bi.intersection(&b_bi).count();
        let union = a_bi.union(&b_bi).count();
        intersection as f64 / union as f64
    } else {
        // Word-level Jaccard for Latin scripts
        let a_words: std::collections::HashSet<_> = a.split_whitespace().collect();
        let b_words: std::collections::HashSet<_> = b.split_whitespace().collect();
        if a_words.is_empty() || b_words.is_empty() {
            return 0.0;
        }
        let intersection = a_words.intersection(&b_words).count();
        let union = a_words.union(&b_words).count();
        intersection as f64 / union as f64
    }
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
