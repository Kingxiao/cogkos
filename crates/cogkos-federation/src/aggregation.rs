use crate::error::{FederationError, Result};
use crate::health::{
    CollectiveIntelligenceHealth, InsightSource, Prediction, ProvenanceInfo,
    calculate_collective_health,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedResult {
    pub query_id: String,
    pub node_results: Vec<NodeResult>,
    pub aggregated: Option<AggregatedResponse>,
    pub metadata: AggregationMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeResult {
    pub node_id: String,
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
    pub response_time_ms: u64,
    pub expertise_score: f64,
}

impl NodeResult {
    /// Convert NodeResult to InsightSource for health check calculation
    pub fn to_insight_source(&self) -> InsightSource {
        let predictions = self
            .data
            .as_ref()
            .map(|data| {
                vec![Prediction {
                    content: data.to_string(),
                    confidence: self.expertise_score,
                }]
            })
            .unwrap_or_default();

        let influence = if self.success {
            self.expertise_score
        } else {
            0.0
        };

        InsightSource {
            source_id: self.node_id.clone(),
            provenance: ProvenanceInfo {
                source_id: self.node_id.clone(),
                source_type: "federated_node".to_string(),
                upstream_sources: vec![],
            },
            influence,
            confidence: self.expertise_score,
            predictions,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedResponse {
    pub content: String,
    pub confidence: f64,
    pub sources: Vec<String>,
    pub coverage_score: f64,
    /// Four-conditions health check result
    pub health: Option<CollectiveIntelligenceHealth>,
}

impl AggregatedResponse {
    /// Create a new aggregated response with health check
    pub fn with_health(
        content: String,
        confidence: f64,
        sources: Vec<String>,
        coverage_score: f64,
        node_results: &[NodeResult],
    ) -> Self {
        let health = Self::calculate_health(node_results);
        Self {
            content,
            confidence,
            sources,
            coverage_score,
            health: Some(health),
        }
    }

    /// Calculate health check from node results
    fn calculate_health(node_results: &[NodeResult]) -> CollectiveIntelligenceHealth {
        let insight_sources: Vec<InsightSource> = node_results
            .iter()
            .filter(|r| r.success)
            .map(|r| r.to_insight_source())
            .collect();

        calculate_collective_health(&insight_sources)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationMetadata {
    pub total_nodes: usize,
    pub successful_nodes: usize,
    pub failed_nodes: usize,
    pub consensus_reached: bool,
    pub consensus_score: Option<f64>,
    pub aggregation_method: String,
    pub processing_time_ms: u64,
}

pub trait ResultAggregator: Send + Sync {
    fn aggregate(&self, results: Vec<NodeResult>) -> Result<AggregatedResponse>;
}

/// Weighted aggregator that considers node expertise and response quality
pub struct WeightedAggregator;

impl WeightedAggregator {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WeightedAggregator {
    fn default() -> Self {
        Self::new()
    }
}

impl ResultAggregator for WeightedAggregator {
    fn aggregate(&self, results: Vec<NodeResult>) -> Result<AggregatedResponse> {
        let successful: Vec<&NodeResult> = results
            .iter()
            .filter(|r| r.success && r.data.is_some())
            .collect();

        if successful.is_empty() {
            return Err(FederationError::AggregationError(
                "No successful results to aggregate".to_string(),
            ));
        }

        // Calculate weights based on expertise scores
        let total_expertise: f64 = successful.iter().map(|r| r.expertise_score).sum();

        let normalized_weights: Vec<f64> = successful
            .iter()
            .map(|r| r.expertise_score / total_expertise.max(0.001))
            .collect();

        // Collect sources
        let sources: Vec<String> = successful.iter().map(|r| r.node_id.clone()).collect();

        // Calculate weighted confidence
        let avg_confidence = successful
            .iter()
            .zip(normalized_weights.iter())
            .map(|(r, w)| r.expertise_score * w)
            .sum::<f64>()
            / normalized_weights.iter().sum::<f64>().max(0.001);

        // Build aggregated content
        let content_parts: Vec<String> = successful
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                r.data.as_ref().map(|d| {
                    let weight = normalized_weights.get(i).copied().unwrap_or(1.0);
                    format!("[Source: {}, Weight: {:.2}] {}", r.node_id, weight, d)
                })
            })
            .collect();

        let aggregated_content = content_parts.join("\n\n");

        // Calculate coverage score based on diversity of sources
        let coverage = (successful.len() as f64 / results.len() as f64).min(1.0);

        let response = AggregatedResponse::with_health(
            aggregated_content,
            avg_confidence.min(1.0),
            sources,
            coverage,
            &results,
        );

        // Log health warnings if any
        if let Some(ref health) = response.health {
            for warning in &health.warnings {
                warn!("Health check warning: {}", warning);
            }
            info!(
                "Four-conditions health: diversity={:.2}, independence={:.2}, decentralization={:.2}, aggregation={:.2}",
                health.diversity_score,
                health.independence_score,
                health.decentralization_score,
                health.aggregation_effectiveness
            );
        }

        Ok(response)
    }
}

/// Consensus aggregator that looks for agreement across nodes
pub struct ConsensusAggregator {
    min_agreement_threshold: f64,
}

impl ConsensusAggregator {
    pub fn new(threshold: f64) -> Self {
        Self {
            min_agreement_threshold: threshold.clamp(0.0, 1.0),
        }
    }

    fn calculate_similarity(a: &str, b: &str) -> f64 {
        // Simple Jaccard similarity for text content
        let words_a: std::collections::HashSet<&str> = a.split_whitespace().collect();
        let words_b: std::collections::HashSet<&str> = b.split_whitespace().collect();

        let intersection: std::collections::HashSet<_> = words_a.intersection(&words_b).collect();
        let union: std::collections::HashSet<_> = words_a.union(&words_b).collect();

        if union.is_empty() {
            0.0
        } else {
            intersection.len() as f64 / union.len() as f64
        }
    }
}

impl ResultAggregator for ConsensusAggregator {
    fn aggregate(&self, results: Vec<NodeResult>) -> Result<AggregatedResponse> {
        let successful: Vec<&NodeResult> = results
            .iter()
            .filter(|r| r.success && r.data.is_some())
            .collect();

        if successful.len() < 2 {
            // Fall back to weighted aggregation for single result
            return WeightedAggregator::new().aggregate(results);
        }

        let contents: Vec<String> = successful
            .iter()
            .filter_map(|r| r.data.as_ref().map(|d| d.to_string()))
            .collect();

        // Calculate pairwise similarities
        let mut agreement_scores = Vec::new();
        for i in 0..contents.len() {
            let mut avg_similarity = 0.0;
            let mut count = 0;
            for j in 0..contents.len() {
                if i != j {
                    avg_similarity += Self::calculate_similarity(&contents[i], &contents[j]);
                    count += 1;
                }
            }
            if count > 0 {
                agreement_scores.push((i, avg_similarity / count as f64));
            }
        }

        // Find results that meet consensus threshold
        let consensus_results: Vec<usize> = agreement_scores
            .iter()
            .filter(|(_, score)| *score >= self.min_agreement_threshold)
            .map(|(idx, _)| *idx)
            .collect();

        if consensus_results.is_empty() {
            return Err(FederationError::ConsensusNotReached);
        }

        // Aggregate consensus results
        let consensus_content: Vec<String> = consensus_results
            .iter()
            .filter_map(|&idx| successful.get(idx))
            .filter_map(|r| r.data.as_ref().map(|d| d.to_string()))
            .collect();

        let sources: Vec<String> = consensus_results
            .iter()
            .filter_map(|&idx| successful.get(idx))
            .map(|r| r.node_id.clone())
            .collect();

        let avg_agreement = agreement_scores
            .iter()
            .filter(|(idx, _)| consensus_results.contains(idx))
            .map(|(_, score)| score)
            .sum::<f64>()
            / consensus_results.len() as f64;

        let coverage = consensus_results.len() as f64 / successful.len() as f64;

        let response = AggregatedResponse::with_health(
            consensus_content.join("\n"),
            avg_agreement,
            sources,
            coverage,
            &results,
        );

        // Log health warnings if any
        if let Some(ref health) = response.health {
            for warning in &health.warnings {
                warn!("Consensus health warning: {}", warning);
            }
        }

        Ok(response)
    }
}

/// Best result aggregator that picks the single best response
pub struct BestResultAggregator;

impl BestResultAggregator {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BestResultAggregator {
    fn default() -> Self {
        Self::new()
    }
}

impl ResultAggregator for BestResultAggregator {
    fn aggregate(&self, results: Vec<NodeResult>) -> Result<AggregatedResponse> {
        let best = results
            .iter()
            .filter(|r| r.success)
            .max_by(|a, b| {
                a.expertise_score
                    .partial_cmp(&b.expertise_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.response_time_ms.cmp(&b.response_time_ms))
            })
            .ok_or_else(|| {
                FederationError::AggregationError("No successful results".to_string())
            })?;

        let content = best
            .data
            .as_ref()
            .map(|d| d.to_string())
            .unwrap_or_default();

        let response = AggregatedResponse::with_health(
            content,
            best.expertise_score,
            vec![best.node_id.clone()],
            1.0 / results.len() as f64,
            &results,
        );

        // Log health warnings if any
        if let Some(ref health) = response.health {
            for warning in &health.warnings {
                warn!("Best result health warning: {}", warning);
            }
        }

        Ok(response)
    }
}

/// Ensemble aggregator that combines multiple strategies
pub struct EnsembleAggregator {
    aggregators: Vec<Box<dyn ResultAggregator>>,
    weights: Vec<f64>,
}

impl EnsembleAggregator {
    pub fn new() -> Self {
        let aggregators: Vec<Box<dyn ResultAggregator>> = vec![
            Box::new(WeightedAggregator::new()),
            Box::new(BestResultAggregator::new()),
        ];

        let weights = vec![0.6, 0.4];

        Self {
            aggregators,
            weights,
        }
    }

    pub fn add_aggregator(&mut self, aggregator: Box<dyn ResultAggregator>, weight: f64) {
        self.aggregators.push(aggregator);
        self.weights.push(weight);
    }
}

impl Default for EnsembleAggregator {
    fn default() -> Self {
        Self::new()
    }
}

impl ResultAggregator for EnsembleAggregator {
    fn aggregate(&self, results: Vec<NodeResult>) -> Result<AggregatedResponse> {
        let mut best_response: Option<AggregatedResponse> = None;
        let mut best_score = 0.0;

        for (i, aggregator) in self.aggregators.iter().enumerate() {
            match aggregator.aggregate(results.clone()) {
                Ok(response) => {
                    let weight = self.weights.get(i).copied().unwrap_or(1.0);
                    let score = response.confidence * response.coverage_score * weight;

                    if score > best_score {
                        best_score = score;
                        best_response = Some(response);
                    }
                }
                Err(e) => {
                    warn!("Aggregator {} failed: {}", i, e);
                }
            }
        }

        best_response
            .ok_or_else(|| FederationError::AggregationError("All aggregators failed".to_string()))
    }
}

/// Smart aggregator that selects strategy based on result characteristics
pub struct SmartAggregator;

impl SmartAggregator {
    pub fn new() -> Self {
        Self
    }

    fn select_strategy(&self, results: &[NodeResult]) -> Box<dyn ResultAggregator> {
        let success_count = results.iter().filter(|r| r.success).count();
        let total = results.len();

        if success_count == 1 {
            // Single result - use best result
            Box::new(BestResultAggregator::new())
        } else if success_count == total {
            // All successful - use weighted
            Box::new(WeightedAggregator::new())
        } else if success_count >= 3 {
            // Multiple results - try consensus
            Box::new(ConsensusAggregator::new(0.5))
        } else {
            // Default to weighted
            Box::new(WeightedAggregator::new())
        }
    }
}

impl Default for SmartAggregator {
    fn default() -> Self {
        Self::new()
    }
}

impl ResultAggregator for SmartAggregator {
    fn aggregate(&self, results: Vec<NodeResult>) -> Result<AggregatedResponse> {
        let strategy = self.select_strategy(&results);
        strategy.aggregate(results)
    }
}

#[derive(Debug, Clone)]
pub struct AggregationConfig {
    pub method: AggregationMethod,
    pub timeout_ms: u64,
    pub require_consensus: bool,
    pub min_success_rate: f64,
}

#[derive(Debug, Clone, Copy)]
pub enum AggregationMethod {
    Weighted,
    Consensus { threshold: f64 },
    BestResult,
    Ensemble,
    Smart,
}

impl Default for AggregationConfig {
    fn default() -> Self {
        Self {
            method: AggregationMethod::Smart,
            timeout_ms: 30000,
            require_consensus: false,
            min_success_rate: 0.5,
        }
    }
}

impl AggregationConfig {
    pub fn create_aggregator(&self) -> Box<dyn ResultAggregator> {
        match self.method {
            AggregationMethod::Weighted => Box::new(WeightedAggregator::new()),
            AggregationMethod::Consensus { threshold } => {
                Box::new(ConsensusAggregator::new(threshold))
            }
            AggregationMethod::BestResult => Box::new(BestResultAggregator::new()),
            AggregationMethod::Ensemble => Box::new(EnsembleAggregator::new()),
            AggregationMethod::Smart => Box::new(SmartAggregator::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_node_result(
        node_id: &str,
        success: bool,
        expertise_score: f64,
        data: Option<serde_json::Value>,
    ) -> NodeResult {
        NodeResult {
            node_id: node_id.to_string(),
            success,
            data,
            error: if success {
                None
            } else {
                Some("Error".to_string())
            },
            response_time_ms: 100,
            expertise_score,
        }
    }

    #[test]
    fn test_weighted_aggregator_with_health_check() {
        let results = vec![
            create_test_node_result("node1", true, 0.9, Some(json!("Response 1"))),
            create_test_node_result("node2", true, 0.8, Some(json!("Response 2"))),
            create_test_node_result("node3", true, 0.7, Some(json!("Response 3"))),
        ];

        let aggregator = WeightedAggregator::new();
        let response = aggregator.aggregate(results).unwrap();

        // Verify health check is present
        assert!(response.health.is_some());
        let health = response.health.unwrap();

        // With 3 diverse sources, diversity should be healthy
        assert!(health.diversity_score > 0.7);
        assert_eq!(
            health.conditions.diversity.status,
            crate::health::HealthStatus::Healthy
        );

        // Independence should be healthy (different source_ids)
        assert!(health.independence_score > 0.7);

        // Decentralization should be healthy (similar influence)
        assert!(health.decentralization_score > 0.7);
    }

    #[test]
    fn test_aggregator_low_diversity_warning() {
        // All results from same source - low diversity
        let results = vec![
            create_test_node_result("node1", true, 0.9, Some(json!("Response 1"))),
            create_test_node_result("node1", true, 0.8, Some(json!("Response 2"))),
            create_test_node_result("node1", true, 0.7, Some(json!("Response 3"))),
        ];

        let aggregator = WeightedAggregator::new();
        let response = aggregator.aggregate(results).unwrap();

        assert!(response.health.is_some());
        let health = response.health.unwrap();

        // Diversity should be unhealthy
        assert_eq!(
            health.conditions.diversity.status,
            crate::health::HealthStatus::Unhealthy
        );
        assert!(health.warnings.iter().any(|w| w.contains("Diversity")));
    }

    #[test]
    fn test_node_result_to_insight_source() {
        let node_result =
            create_test_node_result("test_node", true, 0.85, Some(json!("Test content")));

        let insight = node_result.to_insight_source();

        assert_eq!(insight.source_id, "test_node");
        assert_eq!(insight.confidence, 0.85);
        assert_eq!(insight.influence, 0.85); // Success = expertise_score
        assert_eq!(insight.predictions.len(), 1);
    }

    #[test]
    fn test_failed_node_result_influence() {
        let node_result = create_test_node_result("failed_node", false, 0.9, None);

        let insight = node_result.to_insight_source();

        assert_eq!(insight.influence, 0.0); // Failed = 0 influence
        assert!(insight.predictions.is_empty());
    }

    #[test]
    fn test_best_result_aggregator_with_health() {
        let results = vec![
            create_test_node_result("node1", true, 0.7, Some(json!("Response 1"))),
            create_test_node_result("node2", true, 0.9, Some(json!("Response 2"))),
            create_test_node_result("node3", true, 0.8, Some(json!("Response 3"))),
        ];

        let aggregator = BestResultAggregator::new();
        let response = aggregator.aggregate(results).unwrap();

        assert!(response.health.is_some());
        // Should pick node2 (highest expertise)
        assert!(response.content.contains("Response 2"));
    }

    #[test]
    fn test_consensus_aggregator_with_health() {
        let results = vec![
            create_test_node_result("node1", true, 0.8, Some(json!("The answer is 42"))),
            create_test_node_result("node2", true, 0.8, Some(json!("The answer is 42"))),
            create_test_node_result("node3", true, 0.8, Some(json!("The answer is 42"))),
        ];

        let aggregator = ConsensusAggregator::new(0.5);
        let response = aggregator.aggregate(results).unwrap();

        assert!(response.health.is_some());
        let health = response.health.unwrap();

        // All nodes agree, consensus should be healthy
        assert!(health.aggregation_effectiveness > 0.9);
    }
}
