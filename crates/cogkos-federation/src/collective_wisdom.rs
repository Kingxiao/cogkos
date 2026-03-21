//! Collective Wisdom Health Check Module (Node-level)
//!
//! Evaluates the four conditions of collective wisdom at the **federated node level**:
//! each `NodeResponse` represents a node's reply in a federated query, carrying
//! content, expertise, response time, domain coverage, and provenance metadata.
//!
//! This module uses content-based correlation (Jaccard similarity) for independence,
//! multi-dimensional Gini coefficients for decentralization, configurable thresholds
//! via `HealthCheckConfig`, and consensus-based aggregation effectiveness scoring.
//!
//! For **insight/claim-level** health checks, see [`health`]. That module operates on
//! `InsightSource` objects with simpler provenance grouping and influence-weighted
//! aggregation, suitable for evaluating stored knowledge quality rather than query
//! response quality.
//!
//! Metrics:
//! 1. Diversity - Shannon entropy of response content distribution
//! 2. Independence - pairwise content correlation (lower = more independent)
//! 3. Decentralization - Gini coefficient of expertise distribution
//! 4. Aggregation effectiveness - consensus strength and agreement quality

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Health check result for collective wisdom conditions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectiveWisdomHealth {
    /// Diversity score (0-1, higher is better)
    pub diversity_score: f64,
    /// Independence score (0-1, higher is better)
    pub independence_score: f64,
    /// Decentralization score (0-1, higher is more decentralized)
    pub decentralization_score: f64,
    /// Aggregation effectiveness score (0-1, higher is better)
    pub aggregation_effectiveness: f64,
    /// Overall health score (weighted average)
    pub overall_score: f64,
    /// Detailed metrics
    pub metrics: CollectiveWisdomMetrics,
}

/// Detailed metrics for each condition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectiveWisdomMetrics {
    /// Shannon entropy values per category
    pub entropy_by_category: HashMap<String, f64>,
    /// Provenance independence scores
    pub provenance_scores: Vec<ProvenanceScore>,
    /// Gini coefficient per dimension
    pub gini_coefficients: HashMap<String, f64>,
    /// Aggregation quality metrics
    pub aggregation_metrics: AggregationQualityMetrics,
}

/// Provenance score for a single node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceScore {
    pub node_id: String,
    /// Correlation with other nodes (lower is more independent)
    pub correlation: f64,
    /// Independence score (0-1)
    pub independence: f64,
    pub source_diversity: f64,
}

/// Aggregation quality metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationQualityMetrics {
    /// Consensus strength (0-1)
    pub consensus_strength: f64,
    /// Variance in responses
    pub response_variance: f64,
    /// Coverage ratio
    pub coverage_ratio: f64,
    /// Agreement score
    pub agreement_score: f64,
}

/// Configuration for collective wisdom health check
#[derive(Debug, Clone)]
pub struct HealthCheckConfig {
    /// Minimum diversity threshold (0-1)
    pub min_diversity: f64,
    /// Minimum independence threshold (0-1)
    pub min_independence: f64,
    /// Minimum decentralization threshold (0-1)
    pub min_decentralization: f64,
    /// Minimum aggregation effectiveness threshold (0-1)
    pub min_aggregation_effectiveness: f64,
    /// Weights for overall score calculation
    pub weights: HealthCheckWeights,
}

/// Weights for each condition in overall score
#[derive(Debug, Clone)]
pub struct HealthCheckWeights {
    pub diversity: f64,
    pub independence: f64,
    pub decentralization: f64,
    pub aggregation_effectiveness: f64,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            min_diversity: 0.3,
            min_independence: 0.5,
            min_decentralization: 0.4,
            min_aggregation_effectiveness: 0.5,
            weights: HealthCheckWeights::default(),
        }
    }
}

impl Default for HealthCheckWeights {
    fn default() -> Self {
        Self {
            diversity: 0.25,
            independence: 0.25,
            decentralization: 0.25,
            aggregation_effectiveness: 0.25,
        }
    }
}

/// Collective wisdom health checker
pub struct CollectiveWisdomHealthChecker {
    config: HealthCheckConfig,
}

impl CollectiveWisdomHealthChecker {
    pub fn new(config: HealthCheckConfig) -> Self {
        Self { config }
    }

    pub fn with_default_config() -> Self {
        Self::new(HealthCheckConfig::default())
    }

    /// Perform health check on node responses
    pub fn check(
        &self,
        node_responses: &[NodeResponse],
        aggregation_result: Option<&AggregationResult>,
    ) -> CollectiveWisdomHealth {
        // 1. Calculate diversity (Shannon entropy)
        let diversity_score = self.calculate_diversity(node_responses);
        let entropy_by_category = self.calculate_entropy_by_category(node_responses);

        // 2. Calculate independence (provenance)
        let (independence_score, provenance_scores) = self.calculate_independence(node_responses);

        // 3. Calculate decentralization (Gini coefficient)
        let decentralization_score = self.calculate_decentralization(node_responses);
        let gini_coefficients = self.calculate_gini_by_dimension(node_responses);

        // 4. Calculate aggregation effectiveness
        let (aggregation_effectiveness, aggregation_metrics) =
            self.calculate_aggregation_effectiveness(node_responses, aggregation_result);

        // Calculate overall score
        let overall_score = self.calculate_overall_score(
            diversity_score,
            independence_score,
            decentralization_score,
            aggregation_effectiveness,
        );

        CollectiveWisdomHealth {
            diversity_score,
            independence_score,
            decentralization_score,
            aggregation_effectiveness,
            overall_score,
            metrics: CollectiveWisdomMetrics {
                entropy_by_category,
                provenance_scores,
                gini_coefficients,
                aggregation_metrics,
            },
        }
    }

    /// Calculate diversity using Shannon entropy
    /// Higher entropy = more diversity
    fn calculate_diversity(&self, responses: &[NodeResponse]) -> f64 {
        if responses.is_empty() {
            return 0.0;
        }

        // Calculate entropy based on response content distribution
        let content_counts = self.count_content_distribution(responses);
        let total: f64 = content_counts.values().sum();

        if total == 0.0 {
            return 0.0;
        }

        let entropy: f64 = content_counts
            .values()
            .map(|&count| {
                let p = count as f64 / total;
                if p > 0.0 { -p * p.log2() } else { 0.0 }
            })
            .sum();

        // Normalize to 0-1 (max entropy is log2(n) for n categories)
        let max_entropy = (content_counts.len() as f64).log2().max(1.0);
        (entropy / max_entropy).clamp(0.0, 1.0)
    }

    /// Calculate entropy by category
    fn calculate_entropy_by_category(&self, responses: &[NodeResponse]) -> HashMap<String, f64> {
        let mut category_entropies = HashMap::new();

        for response in responses {
            for (category, value) in &response.category_values {
                let entry = category_entropies
                    .entry(category.clone())
                    .or_insert_with(HashMap::new);
                *entry.entry(value.clone()).or_insert(0.0) += 1.0;
            }
        }

        category_entropies
            .into_iter()
            .map(|(category, counts)| {
                let total: f64 = counts.values().sum();
                let entropy: f64 = counts
                    .values()
                    .map(|&count| {
                        let p = count / total;
                        if p > 0.0 { -p * p.log2() } else { 0.0 }
                    })
                    .sum();
                let max_entropy = (counts.len() as f64).log2().max(1.0);
                (category, (entropy / max_entropy).clamp(0.0, 1.0))
            })
            .collect()
    }

    /// Count content distribution across responses
    fn count_content_distribution(&self, responses: &[NodeResponse]) -> HashMap<String, f64> {
        let mut counts = HashMap::new();
        for response in responses {
            // Use content hash or category as key
            let key = response.content.chars().take(50).collect::<String>();
            *counts.entry(key).or_insert(0.0) += 1.0;
        }
        counts
    }

    /// Calculate independence using provenance analysis
    /// Lower correlation with other nodes = higher independence
    fn calculate_independence(&self, responses: &[NodeResponse]) -> (f64, Vec<ProvenanceScore>) {
        if responses.is_empty() {
            return (0.0, vec![]);
        }

        let n = responses.len();
        let mut provenance_scores = Vec::with_capacity(n);

        // Calculate pairwise correlation
        let correlations = self.calculate_pairwise_correlations(responses);

        for (i, response) in responses.iter().enumerate() {
            let avg_correlation: f64 = if n > 1 {
                correlations
                    .iter()
                    .filter(|(a, b, _)| *a == i || *b == i)
                    .map(|(_, _, corr)| corr)
                    .sum::<f64>()
                    / (n - 1) as f64
            } else {
                0.0
            };

            let independence = (1.0 - avg_correlation).clamp(0.0, 1.0);

            provenance_scores.push(ProvenanceScore {
                node_id: response.node_id.clone(),
                correlation: avg_correlation,
                independence,
                source_diversity: response.source_diversity,
            });
        }

        let avg_independence: f64 = provenance_scores
            .iter()
            .map(|p| p.independence)
            .sum::<f64>()
            / n as f64;

        (avg_independence, provenance_scores)
    }

    /// Calculate pairwise correlations between node responses
    fn calculate_pairwise_correlations(
        &self,
        responses: &[NodeResponse],
    ) -> Vec<(usize, usize, f64)> {
        let mut correlations = Vec::new();
        let n = responses.len();

        for i in 0..n {
            for j in (i + 1)..n {
                let corr = self.correlate_responses(&responses[i], &responses[j]);
                correlations.push((i, j, corr));
            }
        }

        correlations
    }

    /// Calculate correlation between two responses
    fn correlate_responses(&self, a: &NodeResponse, b: &NodeResponse) -> f64 {
        // Simple content-based correlation using common tokens
        let tokens_a: std::collections::HashSet<_> = a.content.split_whitespace().collect();
        let tokens_b: std::collections::HashSet<_> = b.content.split_whitespace().collect();

        if tokens_a.is_empty() || tokens_b.is_empty() {
            return 0.0;
        }

        let intersection: std::collections::HashSet<_> = tokens_a.intersection(&tokens_b).collect();
        let union: std::collections::HashSet<_> = tokens_a.union(&tokens_b).collect();

        if union.is_empty() {
            0.0
        } else {
            intersection.len() as f64 / union.len() as f64
        }
    }

    /// Calculate decentralization using Gini coefficient
    /// Lower Gini = more decentralized
    fn calculate_decentralization(&self, responses: &[NodeResponse]) -> f64 {
        if responses.is_empty() {
            return 0.0;
        }

        // Calculate Gini based on expertise distribution
        let expertises: Vec<f64> = responses.iter().map(|r| r.expertise).collect();
        let gini = self.calculate_gini(&expertises);

        // Invert: lower Gini = higher decentralization
        (1.0 - gini).clamp(0.0, 1.0)
    }

    /// Calculate Gini coefficient
    fn calculate_gini(&self, values: &[f64]) -> f64 {
        if values.is_empty() {
            return 0.0;
        }

        let mut sorted: Vec<f64> = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let n = sorted.len() as f64;
        let sum: f64 = sorted.iter().sum();

        if sum == 0.0 {
            return 0.0;
        }

        let mut numerator = 0.0;
        for (i, &v) in sorted.iter().enumerate() {
            numerator += (2.0 * (i as f64 + 1.0) - n as f64 - 1.0) * v;
        }

        numerator / (n * sum)
    }

    /// Calculate Gini coefficient by dimension
    fn calculate_gini_by_dimension(&self, responses: &[NodeResponse]) -> HashMap<String, f64> {
        let mut gini_by_dim = HashMap::new();

        // Expertise dimension
        let expertises: Vec<f64> = responses.iter().map(|r| r.expertise).collect();
        gini_by_dim.insert("expertise".to_string(), self.calculate_gini(&expertises));

        // Response time dimension
        let times: Vec<f64> = responses
            .iter()
            .map(|r| r.response_time_ms as f64)
            .collect();
        gini_by_dim.insert("response_time".to_string(), self.calculate_gini(&times));

        // Domain coverage dimension
        let domains: Vec<f64> = responses.iter().map(|r| r.domain_coverage as f64).collect();
        gini_by_dim.insert("domain_coverage".to_string(), self.calculate_gini(&domains));

        gini_by_dim
    }

    /// Calculate aggregation effectiveness
    fn calculate_aggregation_effectiveness(
        &self,
        responses: &[NodeResponse],
        aggregation_result: Option<&AggregationResult>,
    ) -> (f64, AggregationQualityMetrics) {
        if responses.is_empty() {
            return (
                0.0,
                AggregationQualityMetrics {
                    consensus_strength: 0.0,
                    response_variance: 0.0,
                    coverage_ratio: 0.0,
                    agreement_score: 0.0,
                },
            );
        }

        // Calculate response variance
        let response_variance = self.calculate_response_variance(responses);

        // Calculate coverage ratio
        let coverage_ratio = responses.len() as f64 / responses.len().max(1) as f64;

        // Calculate agreement score
        let agreement_score = self.calculate_agreement_score(responses);

        // Use aggregation result if available
        let (consensus_strength, confidence) = if let Some(agg) = aggregation_result {
            (agg.consensus_strength, agg.confidence)
        } else {
            // Estimate from responses
            let estimated_consensus = agreement_score;
            let estimated_confidence = 1.0 - response_variance;
            (estimated_consensus, estimated_confidence)
        };

        // Calculate effectiveness score
        let effectiveness =
            (consensus_strength + confidence + coverage_ratio + agreement_score) / 4.0;

        (
            effectiveness.clamp(0.0, 1.0),
            AggregationQualityMetrics {
                consensus_strength,
                response_variance,
                coverage_ratio,
                agreement_score,
            },
        )
    }

    /// Calculate variance in responses
    fn calculate_response_variance(&self, responses: &[NodeResponse]) -> f64 {
        if responses.len() < 2 {
            return 0.0;
        }

        // Calculate pairwise difference as a measure of variance
        let mut total_diff = 0.0;
        let mut count = 0.0;

        for i in 0..responses.len() {
            for j in (i + 1)..responses.len() {
                let diff = 1.0 - self.correlate_responses(&responses[i], &responses[j]);
                total_diff += diff;
                count += 1.0;
            }
        }

        if count > 0.0 { total_diff / count } else { 0.0 }
    }

    /// Calculate agreement score between responses
    fn calculate_agreement_score(&self, responses: &[NodeResponse]) -> f64 {
        if responses.len() < 2 {
            return 1.0;
        }

        let correlations = self.calculate_pairwise_correlations(responses);
        let avg_correlation: f64 =
            correlations.iter().map(|(_, _, corr)| corr).sum::<f64>() / correlations.len() as f64;

        avg_correlation.clamp(0.0, 1.0)
    }

    /// Calculate overall health score
    fn calculate_overall_score(
        &self,
        diversity: f64,
        independence: f64,
        decentralization: f64,
        aggregation_effectiveness: f64,
    ) -> f64 {
        let w = &self.config.weights;
        (diversity * w.diversity
            + independence * w.independence
            + decentralization * w.decentralization
            + aggregation_effectiveness * w.aggregation_effectiveness)
            .clamp(0.0, 1.0)
    }

    /// Check if health meets minimum thresholds
    pub fn is_healthy(&self, health: &CollectiveWisdomHealth) -> bool {
        health.diversity_score >= self.config.min_diversity
            && health.independence_score >= self.config.min_independence
            && health.decentralization_score >= self.config.min_decentralization
            && health.aggregation_effectiveness >= self.config.min_aggregation_effectiveness
    }
}

/// Node response data for health check
#[derive(Debug, Clone)]
pub struct NodeResponse {
    pub node_id: String,
    pub content: String,
    pub expertise: f64,
    pub response_time_ms: u64,
    pub domain_coverage: f64,
    pub source_diversity: f64,
    pub category_values: HashMap<String, String>,
    pub provenance: Vec<String>,
}

/// Aggregation result for comparison
#[derive(Debug, Clone)]
pub struct AggregationResult {
    pub consensus_strength: f64,
    pub confidence: f64,
    pub sources: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_responses() -> Vec<NodeResponse> {
        vec![
            NodeResponse {
                node_id: "node1".to_string(),
                content: "The solution uses machine learning".to_string(),
                expertise: 0.9,
                response_time_ms: 100,
                domain_coverage: 0.8,
                source_diversity: 0.7,
                category_values: HashMap::new(),
                provenance: vec!["source_a".to_string()],
            },
            NodeResponse {
                node_id: "node2".to_string(),
                content: "Machine learning is the key approach".to_string(),
                expertise: 0.8,
                response_time_ms: 120,
                domain_coverage: 0.7,
                source_diversity: 0.6,
                category_values: HashMap::new(),
                provenance: vec!["source_b".to_string()],
            },
            NodeResponse {
                node_id: "node3".to_string(),
                content: "We should apply deep learning techniques".to_string(),
                expertise: 0.85,
                response_time_ms: 90,
                domain_coverage: 0.75,
                source_diversity: 0.8,
                category_values: HashMap::new(),
                provenance: vec!["source_c".to_string()],
            },
        ]
    }

    #[test]
    fn test_diversity_calculation() {
        let checker = CollectiveWisdomHealthChecker::with_default_config();
        let responses = create_test_responses();

        let diversity = checker.calculate_diversity(&responses);
        assert!(diversity > 0.0);
        assert!(diversity <= 1.0);
    }

    #[test]
    fn test_independence_calculation() {
        let checker = CollectiveWisdomHealthChecker::with_default_config();
        let responses = create_test_responses();

        let (independence, provenance_scores) = checker.calculate_independence(&responses);
        assert!(independence >= 0.0);
        assert!(independence <= 1.0);
        assert_eq!(provenance_scores.len(), 3);
    }

    #[test]
    fn test_decentralization_calculation() {
        let checker = CollectiveWisdomHealthChecker::with_default_config();
        let responses = create_test_responses();

        let decentralization = checker.calculate_decentralization(&responses);
        assert!(decentralization >= 0.0);
        assert!(decentralization <= 1.0);
    }

    #[test]
    fn test_gini_coefficient() {
        let checker = CollectiveWisdomHealthChecker::with_default_config();

        // Perfect equality
        let equal = vec![1.0, 1.0, 1.0, 1.0];
        assert!((checker.calculate_gini(&equal)).abs() < 0.01);

        // Perfect inequality
        let unequal = vec![0.0, 0.0, 0.0, 10.0];
        let gini = checker.calculate_gini(&unequal);
        assert!((gini - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_health_check_full() {
        let checker = CollectiveWisdomHealthChecker::with_default_config();
        let responses = create_test_responses();

        let health = checker.check(&responses, None);

        assert!(health.diversity_score >= 0.0);
        assert!(health.independence_score >= 0.0);
        assert!(health.decentralization_score >= 0.0);
        assert!(health.aggregation_effectiveness >= 0.0);
        assert!(health.overall_score >= 0.0);
    }
}
