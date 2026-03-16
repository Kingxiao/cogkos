//! InsightExtractor implementation

use super::*;
use crate::models::{ConflictRecord, ConflictType, ConsolidationStage, EpistemicClaim};
use std::collections::{HashMap, HashSet};

/// Insight extractor for analyzing conflict patterns
pub struct InsightExtractor {
    config: InsightExtractionConfig,
}

impl InsightExtractor {
    pub fn new(config: InsightExtractionConfig) -> Self {
        Self { config }
    }

    /// Check if conditions are met for insight extraction
    pub fn should_extract_insights(
        &self,
        conflicts: &[ConflictRecord],
        total_claims: usize,
    ) -> bool {
        if conflicts.len() < self.config.min_conflicts {
            return false;
        }

        if total_claims < 2 {
            return false;
        }

        let density =
            conflicts.len() as f64 / (total_claims * (total_claims - 1) / 2).max(1) as f64;
        density >= self.config.min_conflict_density
    }

    /// Extract insights from a set of conflicts
    pub fn extract_insights(
        &self,
        conflicts: &[ConflictRecord],
        claims: &HashMap<uuid::Uuid, EpistemicClaim>,
    ) -> Vec<ExtractedInsight> {
        let mut insights = Vec::new();

        // Group conflicts by type using Vec since ConflictType doesn't impl Hash
        let grouped = self.group_by_type(conflicts);

        // Analyze each conflict type
        for (conflict_type, type_conflicts) in grouped {
            if let Some(insight) =
                self.analyze_conflict_type(conflict_type, &type_conflicts, claims)
            {
                insights.push(insight);
            }
        }

        // Look for cross-type patterns
        if let Some(cross_insight) = self.analyze_cross_type_patterns(conflicts, claims) {
            insights.push(cross_insight);
        }

        // Deduplicate similar insights
        self.deduplicate_insights(insights)
    }

    // NOTE: LLM-based insight extraction (extract_insights_with_llm) is planned
    // but requires cogkos_llm dependency. See Phase 3+ roadmap.

    /// Group conflicts by their type
    fn group_by_type<'a>(
        &self,
        conflicts: &'a [ConflictRecord],
    ) -> Vec<(ConflictType, Vec<&'a ConflictRecord>)> {
        let mut groups: Vec<(ConflictType, Vec<&'a ConflictRecord>)> = Vec::new();

        for conflict in conflicts {
            if let Some(group) = groups.iter_mut().find(|(t, _)| *t == conflict.conflict_type) {
                group.1.push(conflict);
            } else {
                groups.push((conflict.conflict_type, vec![conflict]));
            }
        }

        groups
    }

    /// Analyze conflicts of a specific type
    fn analyze_conflict_type(
        &self,
        conflict_type: ConflictType,
        conflicts: &[&ConflictRecord],
        claims: &HashMap<uuid::Uuid, EpistemicClaim>,
    ) -> Option<ExtractedInsight> {
        match conflict_type {
            ConflictType::SourceDisagreement => {
                self.analyze_source_disagreement(conflicts, claims)
            }
            ConflictType::TemporalShift | ConflictType::TemporalInconsistency => {
                self.analyze_temporal_shift(conflicts, claims)
            }
            ConflictType::ContextualDifference | ConflictType::ContextDependent => {
                self.analyze_contextual_difference(conflicts, claims)
            }
            ConflictType::DirectContradiction => {
                self.analyze_direct_contradiction(conflicts, claims)
            }
            ConflictType::ConfidenceMismatch => {
                self.analyze_direct_contradiction(conflicts, claims)
            }
        }
    }

    /// Analyze source disagreement patterns
    fn analyze_source_disagreement(
        &self,
        conflicts: &[&ConflictRecord],
        claims: &HashMap<uuid::Uuid, EpistemicClaim>,
    ) -> Option<ExtractedInsight> {
        if conflicts.len() < 2 {
            return None;
        }

        // Collect unique sources
        let mut sources: HashSet<String> = HashSet::new();
        let mut related_claims: HashSet<uuid::Uuid> = HashSet::new();

        for conflict in conflicts {
            if let Some(claim_a) = claims.get(&conflict.claim_a_id) {
                sources.insert(claim_a.provenance.source_id.clone());
                related_claims.insert(claim_a.id);
            }
            if let Some(claim_b) = claims.get(&conflict.claim_b_id) {
                sources.insert(claim_b.provenance.source_id.clone());
                related_claims.insert(claim_b.id);
            }
        }

        if sources.len() < 2 {
            return None;
        }

        // Extract common entities
        let entities = self.extract_common_entities(conflicts, claims);

        // Calculate confidence based on conflict severity scores
        let avg_severity =
            conflicts.iter().map(|c| c.severity).sum::<f64>() / conflicts.len() as f64;

        let content = format!(
            "Multiple sources ({}) disagree on {}. This may indicate data collection differences, \
             methodological variations, or genuine uncertainty in the domain.",
            sources.len(),
            if entities.is_empty() {
                "related topics".to_string()
            } else {
                format!("the following entities: {}", entities.join(", "))
            }
        );

        Some(ExtractedInsight {
            id: uuid::Uuid::new_v4(),
            content,
            confidence: avg_severity * 0.9, // Slightly reduce confidence for derived insight
            source_conflicts: conflicts.iter().map(|c| c.id).collect(),
            insight_type: InsightType::SourceDiscrepancy,
            supporting_claims: related_claims.into_iter().collect(),
            key_entities: entities,
            temporal_context: None,
        })
    }

    /// Analyze temporal shift patterns
    fn analyze_temporal_shift(
        &self,
        conflicts: &[&ConflictRecord],
        claims: &HashMap<uuid::Uuid, EpistemicClaim>,
    ) -> Option<ExtractedInsight> {
        if conflicts.is_empty() {
            return None;
        }

        // Collect temporal information
        let mut timestamps: Vec<chrono::DateTime<chrono::Utc>> = Vec::new();
        let mut related_claims: HashSet<uuid::Uuid> = HashSet::new();

        for conflict in conflicts {
            if let Some(claim_a) = claims.get(&conflict.claim_a_id) {
                timestamps.push(claim_a.t_valid_start);
                related_claims.insert(claim_a.id);
            }
            if let Some(claim_b) = claims.get(&conflict.claim_b_id) {
                timestamps.push(claim_b.t_valid_start);
                related_claims.insert(claim_b.id);
            }
        }

        if timestamps.len() < 2 {
            return None;
        }

        timestamps.sort();

        // Determine trend direction (simplified)
        let trend = TrendDirection::Unknown; // Would need value extraction for real analysis

        let entities = self.extract_common_entities(conflicts, claims);
        let avg_severity =
            conflicts.iter().map(|c| c.severity).sum::<f64>() / conflicts.len() as f64;

        let content = format!(
            "Changes observed in {} over time ({} to {}). \
             This may represent genuine evolution, market dynamics, or external factors affecting the domain.",
            if entities.is_empty() {
                "related metrics".to_string()
            } else {
                entities.join(", ")
            },
            timestamps.first().unwrap().format("%Y-%m"),
            timestamps.last().unwrap().format("%Y-%m")
        );

        Some(ExtractedInsight {
            id: uuid::Uuid::new_v4(),
            content,
            confidence: avg_severity * 0.85,
            source_conflicts: conflicts.iter().map(|c| c.id).collect(),
            insight_type: InsightType::TemporalEvolution,
            supporting_claims: related_claims.into_iter().collect(),
            key_entities: entities,
            temporal_context: Some(TemporalContext {
                valid_from: timestamps.first().copied(),
                valid_until: timestamps.last().copied(),
                trend_direction: trend,
            }),
        })
    }

    /// Analyze contextual difference patterns
    fn analyze_contextual_difference(
        &self,
        conflicts: &[&ConflictRecord],
        claims: &HashMap<uuid::Uuid, EpistemicClaim>,
    ) -> Option<ExtractedInsight> {
        if conflicts.len() < 2 {
            return None;
        }

        // Collect context information
        let mut contexts: HashSet<String> = HashSet::new();
        let mut related_claims: HashSet<uuid::Uuid> = HashSet::new();

        for conflict in conflicts {
            if let Some(claim_a) = claims.get(&conflict.claim_a_id) {
                contexts.insert(claim_a.tenant_id.clone());
                related_claims.insert(claim_a.id);
            }
            if let Some(claim_b) = claims.get(&conflict.claim_b_id) {
                contexts.insert(claim_b.tenant_id.clone());
                related_claims.insert(claim_b.id);
            }
        }

        if contexts.len() < 2 {
            return None;
        }

        let entities = self.extract_common_entities(conflicts, claims);
        let avg_severity =
            conflicts.iter().map(|c| c.severity).sum::<f64>() / conflicts.len() as f64;

        let content = format!(
            "Differences in {} appear to be context-dependent across {} different scopes. \
             The same entity/claim behaves differently under different conditions, \
             suggesting domain-specific factors or conditional dependencies.",
            if entities.is_empty() {
                "observations".to_string()
            } else {
                entities.join(", ")
            },
            contexts.len()
        );

        Some(ExtractedInsight {
            id: uuid::Uuid::new_v4(),
            content,
            confidence: avg_severity * 0.88,
            source_conflicts: conflicts.iter().map(|c| c.id).collect(),
            insight_type: InsightType::ContextualQualifier,
            supporting_claims: related_claims.into_iter().collect(),
            key_entities: entities,
            temporal_context: None,
        })
    }

    /// Analyze direct contradiction patterns
    fn analyze_direct_contradiction(
        &self,
        conflicts: &[&ConflictRecord],
        claims: &HashMap<uuid::Uuid, EpistemicClaim>,
    ) -> Option<ExtractedInsight> {
        if conflicts.is_empty() {
            return None;
        }

        let mut related_claims: HashSet<uuid::Uuid> = HashSet::new();

        for conflict in conflicts {
            related_claims.insert(conflict.claim_a_id);
            related_claims.insert(conflict.claim_b_id);
        }

        let entities = self.extract_common_entities(conflicts, claims);
        let avg_severity =
            conflicts.iter().map(|c| c.severity).sum::<f64>() / conflicts.len() as f64;

        // High-severity contradictions suggest genuine uncertainty
        let insight_type = if avg_severity > 0.7 {
            InsightType::UncertaintyIndicator
        } else {
            InsightType::SourceDiscrepancy
        };

        let content = format!(
            "Direct contradictions detected regarding {}. \
             This indicates fundamental uncertainty or data quality issues in this domain. \
             Recommend additional verification before acting on these claims.",
            if entities.is_empty() {
                "related topics".to_string()
            } else {
                entities.join(", ")
            }
        );

        Some(ExtractedInsight {
            id: uuid::Uuid::new_v4(),
            content,
            confidence: avg_severity * 0.75, // Lower confidence for contradictions
            source_conflicts: conflicts.iter().map(|c| c.id).collect(),
            insight_type,
            supporting_claims: related_claims.into_iter().collect(),
            key_entities: entities,
            temporal_context: None,
        })
    }

    /// Analyze cross-type patterns
    fn analyze_cross_type_patterns(
        &self,
        conflicts: &[ConflictRecord],
        claims: &HashMap<uuid::Uuid, EpistemicClaim>,
    ) -> Option<ExtractedInsight> {
        // Count distinct conflict types
        let mut seen_types: Vec<ConflictType> = Vec::new();
        for c in conflicts {
            if !seen_types.iter().any(|t| *t == c.conflict_type) {
                seen_types.push(c.conflict_type);
            }
        }

        // If multiple conflict types exist around same entities, suggest emerging pattern
        if seen_types.len() >= 2 && conflicts.len() >= 5 {
            let entities =
                self.extract_common_entities(&conflicts.iter().collect::<Vec<_>>(), claims);

            if !entities.is_empty() {
                let avg_severity =
                    conflicts.iter().map(|c| c.severity).sum::<f64>() / conflicts.len() as f64;

                let content = format!(
                    "Complex conflict pattern emerging around {}: multiple types of \
                     disagreements (temporal, source, contextual) suggest this is an \
                     active area of change or fundamental uncertainty in the domain.",
                    entities.join(", ")
                );

                return Some(ExtractedInsight {
                    id: uuid::Uuid::new_v4(),
                    content,
                    confidence: avg_severity * 0.92,
                    source_conflicts: conflicts.iter().map(|c| c.id).collect(),
                    insight_type: InsightType::EmergingPattern,
                    supporting_claims: conflicts
                        .iter()
                        .flat_map(|c| vec![c.claim_a_id, c.claim_b_id])
                        .collect(),
                    key_entities: entities,
                    temporal_context: None,
                });
            }
        }

        None
    }

    /// Extract common entities from conflicts
    fn extract_common_entities(
        &self,
        conflicts: &[&ConflictRecord],
        claims: &HashMap<uuid::Uuid, EpistemicClaim>,
    ) -> Vec<String> {
        let mut entity_mentions: HashMap<String, usize> = HashMap::new();

        for conflict in conflicts {
            if let Some(claim) = claims.get(&conflict.claim_a_id) {
                for entity in self.extract_entities_from_content(&claim.content) {
                    *entity_mentions.entry(entity).or_insert(0) += 1;
                }
            }
            if let Some(claim) = claims.get(&conflict.claim_b_id) {
                for entity in self.extract_entities_from_content(&claim.content) {
                    *entity_mentions.entry(entity).or_insert(0) += 1;
                }
            }
        }

        // Return entities mentioned in multiple conflicts
        let threshold = conflicts.len() / 2;
        let mut common: Vec<(String, usize)> = entity_mentions
            .into_iter()
            .filter(|(_, count)| *count >= threshold)
            .collect();

        common.sort_by(|a, b| b.1.cmp(&a.1));
        common.into_iter().map(|(e, _)| e).take(5).collect()
    }

    /// Simple entity extraction from content
    fn extract_entities_from_content(&self, content: &str) -> Vec<String> {
        // Simple heuristic: capitalized multi-word phrases and quoted phrases
        let mut entities = Vec::new();

        // Extract quoted phrases
        let mut in_quote = false;
        let mut current_quote = String::new();
        for ch in content.chars() {
            if ch == '"' {
                if in_quote {
                    if current_quote.len() > 2 {
                        entities.push(current_quote.to_lowercase());
                    }
                    current_quote.clear();
                    in_quote = false;
                } else {
                    in_quote = true;
                }
            } else if in_quote {
                current_quote.push(ch);
            }
        }

        // Extract capitalized words (simple heuristic for entities)
        for word in content.split_whitespace() {
            let clean = word.trim_matches(|c: char| !c.is_alphanumeric());
            if clean.len() > 2
                && clean.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                && !["The", "This", "That", "These", "Those", "With", "From", "Into"]
                    .contains(&clean)
            {
                entities.push(clean.to_lowercase());
            }
        }

        entities
    }

    /// Deduplicate similar insights
    fn deduplicate_insights(&self, insights: Vec<ExtractedInsight>) -> Vec<ExtractedInsight> {
        let mut unique: Vec<ExtractedInsight> = Vec::new();

        for insight in insights {
            let is_duplicate = unique
                .iter()
                .any(|u| self.insight_similarity(&insight, u) > 0.8);

            if !is_duplicate {
                unique.push(insight);
            }
        }

        unique
    }

    /// Calculate similarity between two insights
    fn insight_similarity(&self, a: &ExtractedInsight, b: &ExtractedInsight) -> f64 {
        if a.insight_type != b.insight_type {
            return 0.0;
        }

        let content_sim = self.calculate_text_similarity(&a.content, &b.content);
        let entity_sim = self.calculate_jaccard_similarity(
            &a.key_entities.iter().cloned().collect(),
            &b.key_entities.iter().cloned().collect(),
        );

        (content_sim + entity_sim) / 2.0
    }

    /// Calculate text similarity
    fn calculate_text_similarity(&self, a: &str, b: &str) -> f64 {
        let a_words: HashSet<String> = a.split_whitespace().map(|w| w.to_lowercase()).collect();
        let b_words: HashSet<String> = b.split_whitespace().map(|w| w.to_lowercase()).collect();

        self.calculate_jaccard_similarity(&a_words, &b_words)
    }

    /// Calculate Jaccard similarity
    fn calculate_jaccard_similarity<T: std::hash::Hash + Eq>(
        &self,
        a: &HashSet<T>,
        b: &HashSet<T>,
    ) -> f64 {
        if a.is_empty() && b.is_empty() {
            return 1.0;
        }

        let intersection: HashSet<_> = a.intersection(b).collect();
        let union: HashSet<_> = a.union(b).collect();

        intersection.len() as f64 / union.len() as f64
    }
}

/// Elevate insights to Consolidated or Insight stage
pub fn elevate_insight_to_claim(insight: &ExtractedInsight, tenant_id: &str) -> EpistemicClaim {
    use crate::models::{AccessEnvelope, Claimant, NodeType, ProvenanceRecord};

    let mut claim = EpistemicClaim::new(
        insight.content.clone(),
        tenant_id.to_string(),
        NodeType::Insight,
        Claimant::System,
        AccessEnvelope::new(tenant_id),
        ProvenanceRecord::new(
            "insight_extraction".to_string(),
            "evolution_engine".to_string(),
            "insight_extraction".to_string(),
        ),
    );

    claim.confidence = insight.confidence;
    claim.consolidation_stage = ConsolidationStage::Insight;
    claim.derived_from = insight.supporting_claims.clone();

    claim
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    fn create_test_claim(id: uuid::Uuid, content: &str, source: &str) -> EpistemicClaim {
        let mut claim = EpistemicClaim::new(
            content.to_string(),
            "test-tenant".to_string(),
            NodeType::Entity,
            Claimant::System,
            AccessEnvelope::new("test-tenant"),
            ProvenanceRecord::new(
                source.to_string(),
                "test".to_string(),
                "test".to_string(),
            ),
        );
        claim.id = id;
        claim.confidence = 0.8;
        claim
    }

    fn create_test_conflict(
        claim_a: uuid::Uuid,
        claim_b: uuid::Uuid,
        ctype: ConflictType,
    ) -> ConflictRecord {
        let mut conflict = ConflictRecord::new("test-tenant".to_string(), claim_a, claim_b, ctype);
        conflict.severity = 0.75;
        conflict
    }

    #[test]
    fn test_should_extract_insights() {
        let extractor = InsightExtractor::new(InsightExtractionConfig::default());

        // Not enough conflicts
        let few_conflicts: Vec<ConflictRecord> = vec![create_test_conflict(
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            ConflictType::SourceDisagreement,
        )];
        assert!(!extractor.should_extract_insights(&few_conflicts, 10));

        // Enough conflicts
        let many_conflicts: Vec<ConflictRecord> = (0..5)
            .map(|_| {
                create_test_conflict(
                    uuid::Uuid::new_v4(),
                    uuid::Uuid::new_v4(),
                    ConflictType::SourceDisagreement,
                )
            })
            .collect();
        assert!(extractor.should_extract_insights(&many_conflicts, 10));
    }

    #[test]
    fn test_extract_source_discrepancy_insight() {
        let extractor = InsightExtractor::new(InsightExtractionConfig::default());

        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        let id3 = uuid::Uuid::new_v4();

        let claims: HashMap<uuid::Uuid, EpistemicClaim> = [
            (
                id1,
                create_test_claim(id1, "Market share is 25%", "Source A"),
            ),
            (
                id2,
                create_test_claim(id2, "Market share is 30%", "Source B"),
            ),
            (
                id3,
                create_test_claim(id3, "Market share is 28%", "Source C"),
            ),
        ]
        .into_iter()
        .collect();

        let conflicts = vec![
            create_test_conflict(id1, id2, ConflictType::SourceDisagreement),
            create_test_conflict(id2, id3, ConflictType::SourceDisagreement),
            create_test_conflict(id1, id3, ConflictType::SourceDisagreement),
        ];

        let insights = extractor.extract_insights(&conflicts, &claims);
        assert!(!insights.is_empty());
        assert_eq!(insights[0].insight_type, InsightType::SourceDiscrepancy);
    }
}
