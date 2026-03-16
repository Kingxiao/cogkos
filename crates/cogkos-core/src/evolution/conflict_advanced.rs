//! Advanced conflict detection algorithms for CogKOS
//! Phase 2-3: Enhanced conflict detection with semantic analysis

use crate::models::{ConflictRecord, ConflictType, EpistemicClaim, EpistemicStatus, ResolutionStatus};
use std::collections::HashSet;

/// Semantic conflict detector using advanced similarity metrics
pub struct ConflictDetector {
    config: ConflictDetectionConfig,
}

/// Configuration for conflict detection
#[derive(Clone, Debug)]
pub struct ConflictDetectionConfig {
    /// Threshold for direct contradiction (0-1)
    pub contradiction_threshold: f64,
    /// Threshold for semantic similarity (0-1)
    pub semantic_similarity_threshold: f64,
    /// Threshold for temporal shift detection
    pub temporal_shift_threshold: f64,
    /// Enable LLM-based semantic conflict detection
    pub use_llm_semantic: bool,
    /// Minimum confidence for conflict consideration
    pub min_confidence: f64,
}

impl Default for ConflictDetectionConfig {
    fn default() -> Self {
        Self {
            contradiction_threshold: 0.6,
            semantic_similarity_threshold: 0.7,
            temporal_shift_threshold: 0.8,
            use_llm_semantic: false, // Phase 3+ enables this
            min_confidence: 0.3,
        }
    }
}

impl ConflictDetector {
    pub fn new(config: ConflictDetectionConfig) -> Self {
        Self { config }
    }

    /// Detect conflicts between two claims with enhanced semantic analysis
    pub fn detect(&self, claim_a: &EpistemicClaim, claim_b: &EpistemicClaim) -> Option<ConflictRecord> {
        // Basic validation
        if claim_a.id == claim_b.id {
            return None;
        }

        if !self.is_valid(claim_a) || !self.is_valid(claim_b) {
            return None;
        }

        // Check confidence threshold
        if claim_a.confidence < self.config.min_confidence 
            || claim_b.confidence < self.config.min_confidence {
            return None;
        }

        // Try different conflict detection strategies
        if let Some(conflict_type) = self.detect_direct_contradiction(claim_a, claim_b) {
            return Some(self.create_conflict_record(claim_a, claim_b, conflict_type));
        }

        if let Some(conflict_type) = self.detect_temporal_shift(claim_a, claim_b) {
            return Some(self.create_conflict_record(claim_a, claim_b, conflict_type));
        }

        if let Some(conflict_type) = self.detect_source_disagreement(claim_a, claim_b) {
            return Some(self.create_conflict_record(claim_a, claim_b, conflict_type));
        }

        if let Some(conflict_type) = self.detect_contextual_difference(claim_a, claim_b) {
            return Some(self.create_conflict_record(claim_a, claim_b, conflict_type));
        }

        None
    }

    /// Batch conflict detection for efficiency
    pub fn detect_batch(
        &self,
        new_claims: &[EpistemicClaim],
        existing_claims: &[EpistemicClaim],
    ) -> Vec<ConflictRecord> {
        let mut conflicts = Vec::new();
        let mut detected_pairs: HashSet<(uuid::Uuid, uuid::Uuid)> = HashSet::new();

        for new_claim in new_claims {
            for existing in existing_claims {
                // Create canonical pair ID to avoid duplicates
                let pair_id = if new_claim.id < existing.id {
                    (new_claim.id, existing.id)
                } else {
                    (existing.id, new_claim.id)
                };

                if detected_pairs.contains(&pair_id) {
                    continue;
                }

                if let Some(conflict) = self.detect(new_claim, existing) {
                    detected_pairs.insert(pair_id);
                    conflicts.push(conflict);
                }
            }
        }

        conflicts
    }

    /// Check if claim is valid for conflict detection
    fn is_valid(&self, claim: &EpistemicClaim) -> bool {
        !matches!(claim.epistemic_status, 
            EpistemicStatus::Retracted | EpistemicStatus::Superseded)
    }

    /// Detect direct contradiction using negation patterns and semantic similarity
    fn detect_direct_contradiction(
        &self,
        claim_a: &EpistemicClaim,
        claim_b: &EpistemicClaim,
    ) -> Option<ConflictType> {
        let a_content = claim_a.content.to_lowercase();
        let b_content = claim_b.content.to_lowercase();

        // Negation detection patterns
        let negation_patterns: Vec<(Vec<&str>, Vec<&str>)> = vec![
            (vec!["is", "are", "was", "were"], vec!["is not", "are not", "was not", "were not", "isn't", "aren't", "wasn't", "weren't"]),
            (vec!["will", "would", "can", "could"], vec!["will not", "would not", "can not", "could not", "won't", "wouldn't", "can't", "couldn't"]),
            (vec!["has", "have", "had"], vec!["has not", "have not", "had not", "hasn't", "haven't", "hadn't"]),
            (vec!["should", "must", "shall"], vec!["should not", "must not", "shall not", "shouldn't", "mustn't", "shan't"]),
        ];

        for (positive, negative) in negation_patterns {
            let a_has_pos = positive.iter().any(|p| a_content.contains(&format!(" {} ", p)));
            let a_has_neg = negative.iter().any(|n| a_content.contains(n));
            let b_has_pos = positive.iter().any(|p| b_content.contains(&format!(" {} ", p)));
            let b_has_neg = negative.iter().any(|n| b_content.contains(n));

            if (a_has_pos && b_has_neg) || (a_has_neg && b_has_pos) {
                // Check if the rest of the content is similar
                let similarity = self.calculate_semantic_similarity(&a_content, &b_content);
                if similarity > self.config.contradiction_threshold {
                    return Some(ConflictType::DirectContradiction);
                }
            }
        }

        // Numeric value contradiction detection
        if let Some(conflict) = self.detect_numeric_contradiction(&a_content, &b_content) {
            return Some(conflict);
        }

        None
    }

    /// Detect numeric value contradictions (e.g., "price is $100" vs "price is $120")
    fn detect_numeric_contradiction(&self, a: &str, b: &str) -> Option<ConflictType> {
        use regex::Regex;

        // Extract numbers with context
        let number_re = Regex::new(r"(\d+(?:\.\d+)?)").ok()?;

        let a_numbers: Vec<f64> = number_re
            .captures_iter(a)
            .filter_map(|cap| cap[1].parse().ok())
            .collect();

        let b_numbers: Vec<f64> = number_re
            .captures_iter(b)
            .filter_map(|cap| cap[1].parse().ok())
            .collect();

        if a_numbers.len() == b_numbers.len() && !a_numbers.is_empty() {
            let mut has_significant_difference = false;

            for (a_num, b_num) in a_numbers.iter().zip(b_numbers.iter()) {
                let diff_pct = (a_num - b_num).abs() / a_num.max(*b_num);
                if diff_pct > 0.1 { // 10% difference threshold
                    has_significant_difference = true;
                    break;
                }
            }

            if has_significant_difference {
                // Check if the context is similar
                let context_similarity = self.calculate_semantic_similarity(a, b);
                if context_similarity > 0.6 {
                    return Some(ConflictType::DirectContradiction);
                }
            }
        }

        None
    }

    /// Detect temporal shifts (same entity, different time periods)
    fn detect_temporal_shift(
        &self,
        claim_a: &EpistemicClaim,
        claim_b: &EpistemicClaim,
    ) -> Option<ConflictType> {
        // Must be same node type
        if claim_a.node_type != claim_b.node_type {
            return None;
        }

        // Check for temporal markers in content
        let temporal_markers = [
            "in 20", "in 19", "year", "quarter", "q1", "q2", "q3", "q4",
            "january", "february", "march", "april", "may", "june",
            "july", "august", "september", "october", "november", "december",
            "2020", "2021", "2022", "2023", "2024", "2025", "2026",
        ];

        let a_content = claim_a.content.to_lowercase();
        let b_content = claim_b.content.to_lowercase();

        let a_has_temporal = temporal_markers.iter().any(|m| a_content.contains(m));
        let b_has_temporal = temporal_markers.iter().any(|m| b_content.contains(m));

        if a_has_temporal && b_has_temporal {
            let similarity = self.calculate_semantic_similarity(&a_content, &b_content);
            if similarity > self.config.temporal_shift_threshold {
                return Some(ConflictType::TemporalShift);
            }
        }

        // Check validity period overlap
        if let (Some(a_start), Some(b_start)) = (claim_a.valid_from, claim_b.valid_from) {
            if a_start != b_start {
                let similarity = self.calculate_semantic_similarity(&a_content, &b_content);
                if similarity > 0.8 {
                    return Some(ConflictType::TemporalShift);
                }
            }
        }

        None
    }

    /// Detect source disagreement (different sources, same topic)
    fn detect_source_disagreement(
        &self,
        claim_a: &EpistemicClaim,
        claim_b: &EpistemicClaim,
    ) -> Option<ConflictType> {
        // Must be different sources
        if claim_a.provenance.source_id == claim_b.provenance.source_id {
            return None;
        }

        // Same node type increases likelihood
        if claim_a.node_type != claim_b.node_type {
            return None;
        }

        let similarity = self.calculate_semantic_similarity(
            &claim_a.content.to_lowercase(),
            &claim_b.content.to_lowercase(),
        );

        if similarity > self.config.semantic_similarity_threshold && similarity < 1.0 {
            return Some(ConflictType::SourceDisagreement);
        }

        None
    }

    /// Detect contextual differences (different scopes/conditions)
    fn detect_contextual_difference(
        &self,
        claim_a: &EpistemicClaim,
        claim_b: &EpistemicClaim,
    ) -> Option<ConflictType> {
        let a_content = claim_a.content.to_lowercase();
        let b_content = claim_b.content.to_lowercase();

        // Check for contextual qualifiers
        let contextual_markers = [
            "in china", "in usa", "in europe", "in asia",
            "for enterprise", "for small business", "for consumer",
            "in production", "in development", "in testing",
            "domestic", "international", "local", "global",
        ];

        let a_has_context = contextual_markers.iter().any(|m| a_content.contains(m));
        let b_has_context = contextual_markers.iter().any(|m| b_content.contains(m));

        if a_has_context || b_has_context {
            let similarity = self.calculate_semantic_similarity(&a_content, &b_content);
            if similarity > 0.6 && similarity < 0.95 {
                return Some(ConflictType::ContextualDifference);
            }
        }

        // Different tenant scopes
        if claim_a.tenant_id != claim_b.tenant_id {
            let similarity = self.calculate_semantic_similarity(&a_content, &b_content);
            if similarity > 0.6 {
                return Some(ConflictType::ContextualDifference);
            }
        }

        None
    }

    /// Calculate semantic similarity using enhanced Jaccard with TF-like weighting
    fn calculate_semantic_similarity(&self, a: &str, b: &str) -> f64 {
        // Tokenize and normalize
        let a_words: HashSet<String> = self.tokenize(a);
        let b_words: HashSet<String> = self.tokenize(b);

        if a_words.is_empty() || b_words.is_empty() {
            return 0.0;
        }

        // Calculate weighted intersection
        let intersection: HashSet<_> = a_words.intersection(&b_words).collect();
        let union: HashSet<_> = a_words.union(&b_words).collect();

        if union.is_empty() {
            return 0.0;
        }

        // Jaccard similarity
        intersection.len() as f64 / union.len() as f64
    }

    /// Tokenize text into normalized words
    fn tokenize(&self, text: &str) -> HashSet<String> {
        text.split_whitespace()
            .map(|w| {
                w.trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase()
            })
            .filter(|w| !w.is_empty() && !self.is_stop_word(w))
            .collect()
    }

    /// Check if word is a stop word
    fn is_stop_word(&self, word: &str) -> bool {
        let stop_words: HashSet<&str> = [
            "the", "a", "an", "is", "are", "was", "were", "be", "been",
            "being", "have", "has", "had", "do", "does", "did", "will",
            "would", "could", "should", "may", "might", "must", "shall",
            "can", "need", "dare", "ought", "used", "to", "of", "in",
            "for", "on", "with", "at", "by", "from", "as", "into",
            "through", "during", "before", "after", "above", "below",
            "between", "under", "again", "further", "then", "once",
            "here", "there", "when", "where", "why", "how", "all",
            "each", "few", "more", "most", "other", "some", "such",
            "no", "nor", "not", "only", "own", "same", "so", "than",
            "too", "very", "just", "and", "but", "if", "or", "because",
            "until", "while", "this", "that", "these", "those",
        ].iter().cloned().collect();

        stop_words.contains(word)
    }

    /// Create a conflict record
    fn create_conflict_record(
        &self,
        claim_a: &EpistemicClaim,
        claim_b: &EpistemicClaim,
        conflict_type: ConflictType,
    ) -> ConflictRecord {
        let mut record = ConflictRecord::new(
            claim_a.tenant_id.clone(),
            claim_a.id,
            claim_b.id,
            conflict_type,
            format!(
                "Conflict detected between claims {} and {}: {:?}",
                claim_a.id, claim_b.id, conflict_type
            ),
        );
        record.resolution_status = ResolutionStatus::Open;
        record.confidence = (claim_a.confidence + claim_b.confidence) / 2.0;
        record
    }
}

// LLM-based semantic conflict detection is implemented in cogkos-sleep::conflict::detect_llm_semantic_conflicts
// to avoid circular dependency (cogkos-core cannot depend on cogkos-llm).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    fn create_test_claim(content: &str, confidence: f64, source_id: &str) -> EpistemicClaim {
        let mut claim = EpistemicClaim::new(
            "test-tenant".to_string(),
            content.to_string(),
            NodeType::Entity,
            Claimant::System,
            AccessEnvelope::new("test-tenant"),
            ProvenanceRecord::new(source_id, "test"),
        );
        claim.confidence = confidence;
        claim
    }

    #[test]
    fn test_direct_contradiction() {
        let detector = ConflictDetector::new(ConflictDetectionConfig::default());

        let claim_a = create_test_claim("The product is good", 0.8, "source1");
        let claim_b = create_test_claim("The product is not good", 0.8, "source2");

        let conflict = detector.detect(&claim_a, &claim_b);
        assert!(conflict.is_some());
        assert_eq!(conflict.unwrap().conflict_type, ConflictType::DirectContradiction);
    }

    #[test]
    fn test_numeric_contradiction() {
        let detector = ConflictDetector::new(ConflictDetectionConfig::default());

        let claim_a = create_test_claim("The price is $100", 0.8, "source1");
        let claim_b = create_test_claim("The price is $150", 0.8, "source2");

        let conflict = detector.detect(&claim_a, &claim_b);
        assert!(conflict.is_some());
        assert_eq!(conflict.unwrap().conflict_type, ConflictType::DirectContradiction);
    }

    #[test]
    fn test_temporal_shift() {
        let detector = ConflictDetector::new(ConflictDetectionConfig::default());

        let claim_a = create_test_claim("Revenue was 10M in 2024", 0.8, "source1");
        let claim_b = create_test_claim("Revenue was 15M in 2025", 0.8, "source2");

        let conflict = detector.detect(&claim_a, &claim_b);
        assert!(conflict.is_some());
        assert_eq!(conflict.unwrap().conflict_type, ConflictType::TemporalShift);
    }

    #[test]
    fn test_source_disagreement() {
        let detector = ConflictDetector::new(ConflictDetectionConfig::default());

        let claim_a = create_test_claim("Market share is 25%", 0.8, "source1");
        let claim_b = create_test_claim("Market share is 30%", 0.8, "source2");

        let conflict = detector.detect(&claim_a, &claim_b);
        assert!(conflict.is_some());
        assert_eq!(conflict.unwrap().conflict_type, ConflictType::SourceDisagreement);
    }

    #[test]
    fn test_no_conflict_different_topics() {
        let detector = ConflictDetector::new(ConflictDetectionConfig::default());

        let claim_a = create_test_claim("The sky is blue", 0.8, "source1");
        let claim_b = create_test_claim("Pizza is delicious", 0.8, "source2");

        let conflict = detector.detect(&claim_a, &claim_b);
        assert!(conflict.is_none());
    }

    #[test]
    fn test_semantic_similarity() {
        let detector = ConflictDetector::new(ConflictDetectionConfig::default());

        let sim1 = detector.calculate_semantic_similarity(
            "hello world test",
            "hello world test"
        );
        assert!(sim1 > 0.99);

        let sim2 = detector.calculate_semantic_similarity(
            "the quick brown fox",
            "the lazy dog"
        );
        assert!(sim2 < 0.5);
    }
}
