//! Collective Intelligence Health Check - Four Conditions
//!
//! Implements quantitative checks for federated insight quality:
//! 1. Diversity - Shannon entropy of insight source distribution
//! 2. Independence - Provenance-based source independence
//! 3. Decentralization - Gini coefficient of insight influence
//! 4. Aggregation Effectiveness - Aggregated vs best single source prediction

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Health check result for collective intelligence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectiveIntelligenceHealth {
    /// Diversity score (Shannon entropy normalized, 0-1)
    pub diversity_score: f64,
    /// Independence score (provenance independence, 0-1)
    pub independence_score: f64,
    /// Decentralization score (inverse Gini, 0-1 where 1 = fully decentralized)
    pub decentralization_score: f64,
    /// Aggregation effectiveness (aggregated vs best single source)
    pub aggregation_effectiveness: f64,
    /// Overall health score (weighted average)
    pub overall_health: f64,
    /// Detailed condition results
    pub conditions: ConditionResults,
    /// Warnings if any
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionResults {
    pub diversity: DiversityResult,
    pub independence: IndependenceResult,
    pub decentralization: DecentralizationResult,
    pub aggregation: AggregationResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiversityResult {
    pub score: f64,
    pub entropy: f64,
    pub source_count: usize,
    pub source_distribution: HashMap<String, usize>,
    pub status: HealthStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndependenceResult {
    pub score: f64,
    pub independent_sources: usize,
    pub total_sources: usize,
    pub provenance_groups: HashMap<String, Vec<String>>,
    pub status: HealthStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecentralizationResult {
    pub score: f64,
    pub gini_coefficient: f64,
    pub top_influence_share: f64,
    pub insight_count: usize,
    pub status: HealthStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationResult {
    pub score: f64,
    pub aggregated_confidence: f64,
    pub best_single_confidence: f64,
    pub method: String,
    pub status: HealthStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Warning,
    Unhealthy,
}

/// Insight source information for health check
#[derive(Debug, Clone)]
pub struct InsightSource {
    pub source_id: String,
    pub provenance: ProvenanceInfo,
    pub influence: f64,
    pub confidence: f64,
    pub predictions: Vec<Prediction>,
}

#[derive(Debug, Clone)]
pub struct ProvenanceInfo {
    pub source_id: String,
    pub source_type: String,
    pub upstream_sources: Vec<String>,
}

/// Prediction from a source
#[derive(Debug, Clone)]
pub struct Prediction {
    pub content: String,
    pub confidence: f64,
}

impl CollectiveIntelligenceHealth {
    /// Calculate collective intelligence health from insights
    pub fn calculate(insights: &[InsightSource]) -> Self {
        calculate_collective_health(insights)
    }

    /// Check if all four conditions are healthy
    pub fn is_healthy(&self) -> bool {
        self.overall_health >= 0.7
            && self.conditions.diversity.status == HealthStatus::Healthy
            && self.conditions.independence.status == HealthStatus::Healthy
            && self.conditions.decentralization.status == HealthStatus::Healthy
            && self.conditions.aggregation.status == HealthStatus::Healthy
    }

    /// Get summary of health status
    pub fn summary(&self) -> String {
        format!(
            "Four-conditions: diversity={:.2} ({}), independence={:.2} ({}), decentralization={:.2} ({}), aggregation={:.2} ({})",
            self.diversity_score,
            self.conditions.diversity.status.as_str(),
            self.independence_score,
            self.conditions.independence.status.as_str(),
            self.decentralization_score,
            self.conditions.decentralization.status.as_str(),
            self.aggregation_effectiveness,
            self.conditions.aggregation.status.as_str(),
        )
    }
}

impl HealthStatus {
    fn as_str(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "healthy",
            HealthStatus::Warning => "warning",
            HealthStatus::Unhealthy => "unhealthy",
        }
    }
}

/// Calculate collective intelligence health from insights
pub fn calculate_collective_health(insights: &[InsightSource]) -> CollectiveIntelligenceHealth {
    if insights.is_empty() {
        return CollectiveIntelligenceHealth {
            diversity_score: 0.0,
            independence_score: 0.0,
            decentralization_score: 0.0,
            aggregation_effectiveness: 0.0,
            overall_health: 0.0,
            conditions: ConditionResults {
                diversity: DiversityResult {
                    score: 0.0,
                    entropy: 0.0,
                    source_count: 0,
                    source_distribution: HashMap::new(),
                    status: HealthStatus::Unhealthy,
                },
                independence: IndependenceResult {
                    score: 0.0,
                    independent_sources: 0,
                    total_sources: 0,
                    provenance_groups: HashMap::new(),
                    status: HealthStatus::Unhealthy,
                },
                decentralization: DecentralizationResult {
                    score: 0.0,
                    gini_coefficient: 0.0,
                    top_influence_share: 0.0,
                    insight_count: 0,
                    status: HealthStatus::Unhealthy,
                },
                aggregation: AggregationResult {
                    score: 0.0,
                    aggregated_confidence: 0.0,
                    best_single_confidence: 0.0,
                    method: "none".to_string(),
                    status: HealthStatus::Unhealthy,
                },
            },
            warnings: vec!["No insights provided".to_string()],
        };
    }

    // Calculate each condition
    let diversity = calculate_diversity(insights);
    let independence = calculate_independence(insights);
    let decentralization = calculate_decentralization(insights);
    let aggregation = calculate_aggregation_effectiveness(insights);

    // Overall health as weighted average
    let overall_health = diversity.score * 0.25
        + independence.score * 0.25
        + decentralization.score * 0.25
        + aggregation.score * 0.25;

    let mut warnings = Vec::new();

    // Add warnings for unhealthy conditions
    if diversity.status == HealthStatus::Unhealthy {
        warnings.push("Diversity below threshold - knowledge sources too homogeneous".to_string());
    }
    if independence.status == HealthStatus::Unhealthy {
        warnings
            .push("Independence below threshold - sources lack provenance diversity".to_string());
    }
    if decentralization.status == HealthStatus::Unhealthy {
        warnings.push("Decentralization below threshold - few insights dominate".to_string());
    }
    if aggregation.status == HealthStatus::Unhealthy {
        warnings.push(
            "Aggregation effectiveness below threshold - consider different aggregation method"
                .to_string(),
        );
    }

    CollectiveIntelligenceHealth {
        diversity_score: diversity.score,
        independence_score: independence.score,
        decentralization_score: decentralization.score,
        aggregation_effectiveness: aggregation.score,
        overall_health,
        conditions: ConditionResults {
            diversity,
            independence,
            decentralization,
            aggregation,
        },
        warnings,
    }
}

/// Calculate diversity (Shannon entropy)
fn calculate_diversity(insights: &[InsightSource]) -> DiversityResult {
    // Count sources
    let mut source_counts: HashMap<String, usize> = HashMap::new();
    for insight in insights {
        *source_counts.entry(insight.source_id.clone()).or_insert(0) += 1;
    }

    let source_count = source_counts.len();
    let total = insights.len() as f64;

    if total == 0.0 || source_count == 0 {
        return DiversityResult {
            score: 0.0,
            entropy: 0.0,
            source_count: 0,
            source_distribution: HashMap::new(),
            status: HealthStatus::Unhealthy,
        };
    }

    // Calculate Shannon entropy
    let mut entropy = 0.0;
    for count in source_counts.values() {
        let p = *count as f64 / total;
        if p > 0.0 {
            entropy -= p * p.log2();
        }
    }

    // Normalize entropy (max = log2(K) where K = number of sources)
    let max_entropy = (source_count as f64).log2();
    let normalized_entropy = if max_entropy > 0.0 {
        entropy / max_entropy
    } else {
        0.0
    };

    // Health threshold: entropy > 0.7 * max_entropy
    let status = if normalized_entropy > 0.7 {
        HealthStatus::Healthy
    } else if normalized_entropy > 0.4 {
        HealthStatus::Warning
    } else {
        HealthStatus::Unhealthy
    };

    DiversityResult {
        score: normalized_entropy,
        entropy,
        source_count,
        source_distribution: source_counts,
        status,
    }
}

/// Calculate independence (provenance-based)
fn calculate_independence(insights: &[InsightSource]) -> IndependenceResult {
    let total_sources = insights.len();

    if total_sources == 0 {
        return IndependenceResult {
            score: 0.0,
            independent_sources: 0,
            total_sources: 0,
            provenance_groups: HashMap::new(),
            status: HealthStatus::Unhealthy,
        };
    }

    // Group sources by provenance
    let mut provenance_groups: HashMap<String, Vec<String>> = HashMap::new();
    for insight in insights {
        let key = format!(
            "{}_{}",
            insight.provenance.source_type, insight.provenance.source_id
        );
        provenance_groups
            .entry(key)
            .or_default()
            .push(insight.source_id.clone());
    }

    // Count independent sources (unique provenance roots)
    let independent_sources = provenance_groups.len();

    // Calculate independence score
    let score = if total_sources > 1 {
        independent_sources as f64 / total_sources as f64
    } else {
        1.0
    };

    let status = if score > 0.7 {
        HealthStatus::Healthy
    } else if score > 0.4 {
        HealthStatus::Warning
    } else {
        HealthStatus::Unhealthy
    };

    IndependenceResult {
        score,
        independent_sources,
        total_sources,
        provenance_groups,
        status,
    }
}

/// Calculate decentralization (Gini coefficient of influence)
fn calculate_decentralization(insights: &[InsightSource]) -> DecentralizationResult {
    let insight_count = insights.len();

    if insight_count == 0 {
        return DecentralizationResult {
            score: 0.0,
            gini_coefficient: 0.0,
            top_influence_share: 0.0,
            insight_count: 0,
            status: HealthStatus::Unhealthy,
        };
    }

    // Extract influence values
    let mut influences: Vec<f64> = insights.iter().map(|i| i.influence).collect();
    influences.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Calculate Gini coefficient
    let n = influences.len() as f64;
    let sum: f64 = influences.iter().sum();
    let gini = if sum > 0.0 {
        let mut cumsum = 0.0;
        let mut gini_sum = 0.0;
        for &val in influences.iter() {
            cumsum += val;
            gini_sum += cumsum;
        }
        1.0 - (2.0 * gini_sum / (n * sum)) + 1.0 / n
    } else {
        0.0
    };

    // Calculate top influence share
    let total_influence: f64 = influences.iter().sum();
    let top_share = if total_influence > 0.0 {
        influences
            .last()
            .map(|&v| v / total_influence)
            .unwrap_or(0.0)
    } else {
        0.0
    };

    // Score: 1 - Gini (so 1 = fully decentralized, 0 = fully centralized)
    let score = 1.0 - gini;

    // Health threshold: Gini < 0.3 (score > 0.7)
    let status = if gini < 0.3 {
        HealthStatus::Healthy
    } else if gini < 0.5 {
        HealthStatus::Warning
    } else {
        HealthStatus::Unhealthy
    };

    DecentralizationResult {
        score,
        gini_coefficient: gini,
        top_influence_share: top_share,
        insight_count,
        status,
    }
}

/// Calculate aggregation effectiveness
fn calculate_aggregation_effectiveness(insights: &[InsightSource]) -> AggregationResult {
    if insights.is_empty() {
        return AggregationResult {
            score: 0.0,
            aggregated_confidence: 0.0,
            best_single_confidence: 0.0,
            method: "none".to_string(),
            status: HealthStatus::Unhealthy,
        };
    }

    // Find best single source confidence
    let best_single = insights
        .iter()
        .max_by(|a, b| {
            a.confidence
                .partial_cmp(&b.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|i| i.confidence)
        .unwrap_or(0.0);

    // Calculate aggregated confidence (weighted average by influence)
    let total_influence: f64 = insights.iter().map(|i| i.influence).sum();
    let aggregated = if total_influence > 0.0 {
        insights
            .iter()
            .map(|i| i.confidence * i.influence)
            .sum::<f64>()
            / total_influence
    } else {
        // Simple average if no influence data
        insights.iter().map(|i| i.confidence).sum::<f64>() / insights.len() as f64
    };

    // Calculate effectiveness ratio
    let score = if best_single > 0.0 {
        aggregated / best_single
    } else {
        0.0
    };

    // Health threshold: aggregated >= best single (score >= 1.0)
    // We allow some tolerance
    let status = if score >= 0.95 {
        HealthStatus::Healthy
    } else if score >= 0.8 {
        HealthStatus::Warning
    } else {
        HealthStatus::Unhealthy
    };

    AggregationResult {
        score: score.min(1.0), // Cap at 1.0
        aggregated_confidence: aggregated,
        best_single_confidence: best_single,
        method: "weighted_average".to_string(),
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_insight(
        source_id: &str,
        source_type: &str,
        influence: f64,
        confidence: f64,
    ) -> InsightSource {
        InsightSource {
            source_id: source_id.to_string(),
            provenance: ProvenanceInfo {
                source_id: source_id.to_string(),
                source_type: source_type.to_string(),
                upstream_sources: vec![],
            },
            influence,
            confidence,
            predictions: vec![],
        }
    }

    #[test]
    fn test_diversity_healthy() {
        // Multiple sources with even distribution
        let insights = vec![
            create_test_insight("source_a", "agent", 0.5, 0.8),
            create_test_insight("source_b", "human", 0.5, 0.7),
            create_test_insight("source_c", "system", 0.5, 0.9),
            create_test_insight("source_d", "external", 0.5, 0.6),
        ];

        let result = calculate_diversity(&insights);
        assert!(result.score > 0.9);
        assert_eq!(result.status, HealthStatus::Healthy);
    }

    #[test]
    fn test_diversity_unhealthy() {
        // Single source
        let insights = vec![
            create_test_insight("source_a", "agent", 0.5, 0.8),
            create_test_insight("source_a", "agent", 0.5, 0.7),
            create_test_insight("source_a", "agent", 0.5, 0.9),
        ];

        let result = calculate_diversity(&insights);
        assert_eq!(result.status, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_independence() {
        let insights = vec![
            create_test_insight("source_a", "type_a", 0.5, 0.8),
            create_test_insight("source_b", "type_b", 0.5, 0.7),
            create_test_insight("source_c", "type_a", 0.5, 0.9), // Same type but different source_id
        ];

        let result = calculate_independence(&insights);
        // Should have 3 independent provenance groups (each unique source_type+source_id combo)
        assert_eq!(result.independent_sources, 3);
        assert!(result.score > 0.6);
    }

    #[test]
    fn test_independence_same_source() {
        // Multiple insights from the same source should count as 1
        let insights = vec![
            create_test_insight("source_a", "type_a", 0.5, 0.8),
            create_test_insight("source_a", "type_a", 0.5, 0.7),
            create_test_insight("source_a", "type_a", 0.5, 0.9),
        ];

        let result = calculate_independence(&insights);
        assert_eq!(result.independent_sources, 1);
        assert_eq!(result.status, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_decentralization_healthy() {
        // Even influence distribution
        let insights = vec![
            create_test_insight("source_a", "type_a", 0.25, 0.8),
            create_test_insight("source_b", "type_b", 0.25, 0.7),
            create_test_insight("source_c", "type_c", 0.25, 0.9),
            create_test_insight("source_d", "type_d", 0.25, 0.6),
        ];

        let result = calculate_decentralization(&insights);
        assert!(result.score > 0.9);
        assert!(result.gini_coefficient < 0.1);
    }

    #[test]
    fn test_decentralization_unhealthy() {
        // One dominant source
        let insights = vec![
            create_test_insight("source_a", "type_a", 0.9, 0.8),
            create_test_insight("source_b", "type_b", 0.03, 0.7),
            create_test_insight("source_c", "type_c", 0.03, 0.9),
            create_test_insight("source_d", "type_d", 0.04, 0.6),
        ];

        let result = calculate_decentralization(&insights);
        assert!(result.gini_coefficient > 0.5);
        assert_eq!(result.status, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_aggregation_effectiveness() {
        let insights = vec![
            create_test_insight("source_a", "type_a", 0.5, 0.8),
            create_test_insight("source_b", "type_b", 0.5, 0.7),
        ];

        let result = calculate_aggregation_effectiveness(&insights);
        // Aggregated (0.75) should be close to or better than best single (0.8)
        assert!(result.score > 0.9);
    }

    #[test]
    fn test_empty_insights() {
        let insights: Vec<InsightSource> = vec![];
        let health = calculate_collective_health(&insights);

        assert_eq!(health.overall_health, 0.0);
        assert!(
            health
                .warnings
                .contains(&"No insights provided".to_string())
        );
    }

    #[test]
    fn test_overall_health() {
        let insights = vec![
            create_test_insight("source_a", "type_a", 0.5, 0.8),
            create_test_insight("source_b", "type_b", 0.5, 0.7),
        ];

        let health = calculate_collective_health(&insights);
        assert!(health.overall_health > 0.0);
        assert!(health.conditions.diversity.status == HealthStatus::Healthy);
    }
}
