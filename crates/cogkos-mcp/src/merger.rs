//! Result merger for combining vector search and graph activation diffusion results

use cogkos_core::authority::AuthorityTier;
use cogkos_core::models::{EpistemicClaim, GraphNode, GraphRelation, Id, VectorMatch};

/// Configuration for the merge algorithm
#[derive(Debug, Clone)]
pub struct MergeConfig {
    /// Weight for vector similarity score (0.0-1.0)
    pub vector_weight: f64,
    /// Weight for graph activation value (0.0-1.0)
    pub graph_weight: f64,
    /// Weight for authority tier boost (0.0-1.0)
    pub authority_weight: f64,
    /// Weight for feedback quality signal (0.0-1.0)
    pub feedback_weight: f64,
    /// Minimum combined score to include in results
    pub min_score: f64,
    /// Maximum results to return
    pub max_results: usize,
}

impl Default for MergeConfig {
    fn default() -> Self {
        let authority_weight: f64 = std::env::var("MERGE_AUTHORITY_WEIGHT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.2);
        let feedback_weight: f64 = std::env::var("MERGE_FEEDBACK_WEIGHT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.25);
        let vector_weight: f64 = std::env::var("MERGE_VECTOR_WEIGHT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.3);
        let graph_weight: f64 = std::env::var("MERGE_GRAPH_WEIGHT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.35);
        Self {
            vector_weight,
            graph_weight,
            authority_weight,
            feedback_weight,
            min_score: 0.1,
            max_results: 20,
        }
    }
}

/// Merged result item
#[derive(Debug, Clone)]
pub struct MergedResult {
    pub claim_id: Id,
    pub content: String,
    pub combined_score: f64,
    pub vector_score: Option<f64>,
    pub graph_activation: Option<f64>,
    pub confidence: f64,
    pub source: ResultSource,
}

/// Source of the result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultSource {
    /// Only found in vector search
    VectorOnly,
    /// Only found in graph diffusion
    GraphOnly,
    /// Found in both
    Both,
}

/// Merge vector search results with graph activation diffusion results
///
/// # Arguments
/// * `vector_matches` - Results from vector similarity search
/// * `graph_nodes` - Results from graph activation diffusion
/// * `claims` - Full claim objects for vector matches
/// * `config` - Merge configuration
///
/// # Returns
/// * Sorted vector of merged results (highest score first)
/// * Vector of graph relations for the response
pub fn merge_results(
    vector_matches: &[VectorMatch],
    graph_nodes: &[GraphNode],
    claims: &[(Id, EpistemicClaim)],
    config: &MergeConfig,
) -> (Vec<MergedResult>, Vec<GraphRelation>) {
    // Build results map from scratch
    let mut results: std::collections::HashMap<Id, MergedResult> = std::collections::HashMap::new();

    // Process vector search results
    for vm in vector_matches {
        if let Some(claim) = claims.iter().find(|(_, c)| c.id == vm.id).map(|(_, c)| c) {
            let authority_boost = AuthorityTier::resolve(claim).score_boost();
            let feedback_quality = (claim.confidence
                * (claim.activation_weight / 0.5_f64.max(claim.activation_weight)))
            .clamp(0.0, 1.0);
            let remaining_weight = 1.0 - config.authority_weight - config.feedback_weight;
            // Fix: for vector-only results, graph component is 0 (no graph activation)
            // Previously used claim.confidence as graph proxy, which let high-confidence
            // irrelevant claims outrank semantically relevant ones
            let combined = vm.score * config.vector_weight * remaining_weight
                + authority_boost * config.authority_weight
                + feedback_quality * config.feedback_weight;

            results.insert(
                vm.id,
                MergedResult {
                    claim_id: vm.id,
                    content: claim.content.clone(),
                    combined_score: combined,
                    vector_score: Some(vm.score),
                    graph_activation: None,
                    confidence: claim.confidence,
                    source: ResultSource::VectorOnly,
                },
            );
        }
    }

    // Process graph diffusion results
    for gn in graph_nodes {
        let source = if results.contains_key(&gn.id) {
            ResultSource::Both
        } else {
            ResultSource::GraphOnly
        };

        // Find the claim to get confidence
        let confidence = claims
            .iter()
            .find(|(_, c)| c.id == gn.id)
            .map(|(_, c)| c.confidence)
            .unwrap_or(0.5);

        // Calculate combined score using activation, confidence, authority, and feedback quality
        let (authority_boost, feedback_quality) = claims
            .iter()
            .find(|(_, c)| c.id == gn.id)
            .map(|(_, c)| {
                let ab = AuthorityTier::resolve(c).score_boost();
                let fq = (c.confidence * (c.activation_weight / 0.5_f64.max(c.activation_weight)))
                    .clamp(0.0, 1.0);
                (ab, fq)
            })
            .unwrap_or((0.0, 0.0));
        let remaining_weight = 1.0 - config.authority_weight - config.feedback_weight;
        let combined = gn.activation * config.graph_weight * remaining_weight
            + confidence * config.vector_weight * remaining_weight
            + authority_boost * config.authority_weight
            + feedback_quality * config.feedback_weight;

        if let Some(existing) = results.get_mut(&gn.id) {
            // Already exists - update to Both and combine scores
            existing.source = ResultSource::Both;
            existing.graph_activation = Some(gn.activation);
            existing.combined_score = (existing.combined_score + combined) / 2.0;
        } else {
            results.insert(
                gn.id,
                MergedResult {
                    claim_id: gn.id,
                    content: gn.content.clone(),
                    combined_score: combined,
                    vector_score: None,
                    graph_activation: Some(gn.activation),
                    confidence,
                    source,
                },
            );
        }
    }

    // Filter and sort results
    let mut merged: Vec<MergedResult> = results
        .into_values()
        .filter(|r| r.combined_score >= config.min_score)
        .collect();

    // Sort by combined score descending
    merged.sort_by(|a, b| b.combined_score.partial_cmp(&a.combined_score).unwrap());

    // Limit results
    merged.truncate(config.max_results);

    // Build graph relations for response
    let graph_relations: Vec<GraphRelation> = merged
        .iter()
        .filter(|r| r.source != ResultSource::VectorOnly)
        .map(|r| GraphRelation {
            content: strip_frontmatter(&r.content),
            relation_type: if r.source == ResultSource::Both {
                "RELATED".to_string()
            } else {
                "ACTIVATED".to_string()
            },
            activation: r.graph_activation.unwrap_or(r.combined_score),
            source_claim_id: r.claim_id,
        })
        .collect();

    (merged, graph_relations)
}

/// Strip YAML frontmatter (---\n...\n---) from content
fn strip_frontmatter(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.starts_with("---") {
        if let Some(end_pos) = trimmed[3..].find("\n---") {
            let after = &trimmed[3 + end_pos + 4..];
            return after.trim_start_matches('\n').to_string();
        }
    }
    content.to_string()
}

/// Deduplicate results by claim ID, keeping the one with higher score
pub fn deduplicate_results(results: &mut Vec<MergedResult>) {
    let mut seen = std::collections::HashSet::new();
    results.retain(|r| seen.insert(r.claim_id));
}

/// Calculate weighted score combining multiple signals
pub fn calculate_weighted_score(
    vector_score: Option<f64>,
    activation: Option<f64>,
    confidence: f64,
    config: &MergeConfig,
) -> f64 {
    let vs = vector_score.unwrap_or(0.0);
    let act = activation.unwrap_or(0.0);

    vs * config.vector_weight + act * config.graph_weight + confidence * 0.2
}

#[cfg(test)]
mod tests {
    use super::*;
    use cogkos_core::models::{
        AccessEnvelope, Claimant, ConsolidationStage, EpistemicStatus, KnowledgeType, NodeType,
        ProvenanceRecord,
    };
    use uuid::Uuid;

    fn create_test_claim(id: Uuid, content: &str, confidence: f64) -> EpistemicClaim {
        use chrono::Utc;

        EpistemicClaim {
            id,
            tenant_id: "test".to_string(),
            content: content.to_string(),
            node_type: NodeType::Entity,
            knowledge_type: KnowledgeType::Experiential,
            structured_content: None,
            claimant: Claimant::Human {
                user_id: "test".to_string(),
                role: "tester".to_string(),
            },
            epistemic_status: EpistemicStatus::Asserted,
            confidence,
            consolidation_stage: ConsolidationStage::FastTrack,
            version: 1,
            durability: 1.0,
            activation_weight: 1.0,
            access_count: 0,
            last_accessed: None,
            t_valid_start: Utc::now(),
            t_valid_end: None,
            t_known: Utc::now(),
            access_envelope: AccessEnvelope::new("test"),
            provenance: ProvenanceRecord {
                source_id: "test".to_string(),
                source_type: "test".to_string(),
                ingestion_method: "test".to_string(),
                original_url: None,
                audit_hash: "test".to_string(),
            },
            vector_id: None,
            last_prediction_error: None,
            derived_from: vec![],
            superseded_by: None,
            entity_refs: vec![],
            needs_revalidation: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            metadata: serde_json::Map::new(),
        }
    }

    #[test]
    fn test_merge_results_basic() {
        let claim_id1 = Uuid::new_v4();
        let claim_id2 = Uuid::new_v4();
        let claim_id3 = Uuid::new_v4();

        let vector_matches = vec![
            VectorMatch {
                id: claim_id1,
                score: 0.9,
            },
            VectorMatch {
                id: claim_id2,
                score: 0.7,
            },
        ];

        let graph_nodes = vec![
            GraphNode {
                id: claim_id2,
                content: "Graph content 2".to_string(),
                activation: 0.8,
            },
            GraphNode {
                id: claim_id3,
                content: "Graph content 3".to_string(),
                activation: 0.6,
            },
        ];

        let claims = vec![
            (
                claim_id1,
                create_test_claim(claim_id1, "Vector content 1", 0.8),
            ),
            (
                claim_id2,
                create_test_claim(claim_id2, "Shared content 2", 0.7),
            ),
            (claim_id3, create_test_claim(claim_id3, "Graph only 3", 0.6)),
        ];

        let config = MergeConfig::default();
        let (merged, relations) = merge_results(&vector_matches, &graph_nodes, &claims, &config);

        // Should have 3 results
        assert_eq!(merged.len(), 3);

        // First result should be claim_id1 (highest combined score)
        assert_eq!(merged[0].claim_id, claim_id1);

        // Should have graph relations for items from graph
        assert!(relations.len() >= 2);

        // Test deduplication
        let mut test_results = merged.clone();
        deduplicate_results(&mut test_results);
        assert_eq!(test_results.len(), merged.len()); // No duplicates
    }

    #[test]
    fn test_weighted_score_calculation() {
        let config = MergeConfig::default();

        let score1 = calculate_weighted_score(Some(0.9), Some(0.8), 0.7, &config);
        assert!(score1 > 0.0);

        let score2 = calculate_weighted_score(Some(0.9), None, 0.7, &config);
        assert!(score2 < score1); // Lower when no activation

        let score3 = calculate_weighted_score(None, Some(0.8), 0.7, &config);
        assert!(score3 < score1); // Lower when no vector score
    }
}
