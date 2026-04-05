//! Health Check Module - Four-condition health checking for federation
//!
//! Implements the four required health checks for federation:
//! 1. Diversity Entropy - measures knowledge diversity
//! 2. Gini Coefficient - measures concentration/inequality
//! 3. Cascade Detection - detects cascading failure risks
//! 4. Aggregation Quality - measures federation output quality
//!
//! Plus framework health monitoring:
//! 5. Framework monitoring - track prediction power and biases

pub mod aggregation_quality;
pub mod cascade_detection;
pub mod diversity_entropy;
pub mod framework_monitoring;
pub mod gini_coefficient;

pub use aggregation_quality::*;
pub use cascade_detection::*;
pub use diversity_entropy::*;
pub use framework_monitoring::*;
pub use gini_coefficient::*;

use std::collections::HashMap;

/// Complete federation health check result
#[derive(Debug, Clone)]
pub struct FederationHealth {
    /// Diversity metrics
    pub diversity: DiversityHealthResult,
    /// Centralization metrics
    pub centralization: CentralizationResult,
    /// Cascade risk assessment
    pub cascade_risk: CascadeRiskAssessment,
    /// Aggregation quality
    pub aggregation_quality: Option<AggregationQualityMetrics>,
    /// Overall health score (0.0 - 1.0)
    pub overall_health: f64,
    /// Health status
    pub status: OverallHealthStatus,
    /// Timestamp of check
    pub checked_at: chrono::DateTime<chrono::Utc>,
    /// Combined recommendations
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverallHealthStatus {
    Healthy,
    Degraded,
    AtRisk,
    Critical,
}

impl FederationHealth {
    /// Check if federation is healthy
    pub fn is_healthy(&self) -> bool {
        self.status == OverallHealthStatus::Healthy
    }

    /// Check if federation is at risk
    pub fn is_at_risk(&self) -> bool {
        matches!(
            self.status,
            OverallHealthStatus::AtRisk | OverallHealthStatus::Critical
        )
    }

    /// Get critical issues
    pub fn critical_issues(&self) -> Vec<String> {
        let mut issues = Vec::new();

        if self.diversity.status == DiversityStatus::Critical {
            issues.push(format!(
                "Critical diversity: {}",
                self.diversity.warnings.join(", ")
            ));
        }
        if self.centralization.risk_level == RiskLevel::Critical {
            issues.push("Critical centralization risk".to_string());
        }
        if !self.cascade_risk.single_points_of_failure.is_empty() {
            issues.push(format!(
                "Single points of failure: {:?}",
                self.cascade_risk.single_points_of_failure
            ));
        }

        issues
    }
}

/// Federation health checker
pub struct FederationHealthChecker {
    diversity_tracker: DiversityTracker,
    gini_tracker: GiniTracker,
    quality_tracker: AggregationQualityTracker,
    dependency_graph: DependencyGraph,
}

impl FederationHealthChecker {
    pub fn new() -> Self {
        Self {
            diversity_tracker: DiversityTracker::new(),
            gini_tracker: GiniTracker::new(),
            quality_tracker: AggregationQualityTracker::new(),
            dependency_graph: DependencyGraph::new(),
        }
    }

    /// Run complete health check
    pub fn check_health(
        &mut self,
        node_domains: &HashMap<String, Vec<String>>,
        node_expertise: &HashMap<String, HashMap<String, f64>>,
        knowledge_per_node: &[usize],
        queries_per_node: &[u64],
    ) -> FederationHealth {
        // Run individual checks
        let diversity = calculate_diversity_health(node_domains, node_expertise);
        self.diversity_tracker.record(&diversity);

        // Get expertise scores for Gini
        let expertise_scores: Vec<f64> = node_expertise
            .values()
            .map(|exp| exp.values().sum::<f64>() / exp.len().max(1) as f64)
            .collect();

        let centralization =
            analyze_centralization(knowledge_per_node, queries_per_node, &expertise_scores);
        self.gini_tracker.record(&centralization);

        let cascade_risk = assess_cascade_risk(&self.dependency_graph);

        let aggregation_quality = self.quality_tracker.average_quality(100);

        // Calculate overall health
        let diversity_score = diversity.score;
        let centralization_score = 1.0 - centralization.gini; // Invert: lower Gini is better
        let cascade_score = 1.0 - cascade_risk.overall_risk; // Invert: lower risk is better
        let quality_score = aggregation_quality
            .as_ref()
            .map(|q| q.overall_score)
            .unwrap_or(0.5);

        let overall_health = (diversity_score * 0.25)
            + (centralization_score * 0.25)
            + (cascade_score * 0.25)
            + (quality_score * 0.25);

        // Determine status
        let status =
            if overall_health >= 0.7 && diversity.is_healthy() && !centralization.is_at_risk() {
                OverallHealthStatus::Healthy
            } else if overall_health >= 0.5 {
                OverallHealthStatus::Degraded
            } else if overall_health >= 0.3 {
                OverallHealthStatus::AtRisk
            } else {
                OverallHealthStatus::Critical
            };

        // Combine recommendations
        let mut recommendations = Vec::new();
        recommendations.extend(diversity.warnings.clone());
        recommendations.extend(centralization.recommendations.clone());
        recommendations.extend(cascade_risk.recommendations.clone());

        FederationHealth {
            diversity,
            centralization,
            cascade_risk,
            aggregation_quality,
            overall_health,
            status,
            checked_at: chrono::Utc::now(),
            recommendations,
        }
    }

    /// Update dependency graph
    pub fn update_dependency_graph(&mut self, graph: DependencyGraph) {
        self.dependency_graph = graph;
    }

    /// Record aggregation quality
    pub fn record_aggregation_quality(
        &mut self,
        metrics: AggregationQualityMetrics,
        query_type: impl Into<String>,
    ) {
        self.quality_tracker.record(metrics, query_type);
    }

    /// Check if health is declining
    pub fn is_health_declining(&self) -> bool {
        let diversity_declining = self.diversity_tracker.trend(10) < -0.05;
        let centralization_worsening = self.gini_tracker.is_worsening(10);
        let quality_declining = self.quality_tracker.is_declining(10);

        diversity_declining || centralization_worsening || quality_declining
    }
}

impl Default for FederationHealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_federation_health_checker() {
        let mut checker = FederationHealthChecker::new();

        let mut node_domains = HashMap::new();
        node_domains.insert("node1".to_string(), vec!["tech".to_string()]);
        node_domains.insert("node2".to_string(), vec!["business".to_string()]);

        let node_expertise = HashMap::new();
        let knowledge = vec![100, 100];
        let queries = vec![1000, 1000];

        let health = checker.check_health(&node_domains, &node_expertise, &knowledge, &queries);

        assert!(health.overall_health >= 0.0 && health.overall_health <= 1.0);
    }

    #[test]
    fn test_overall_health_status() {
        let health = FederationHealth {
            diversity: DiversityHealthResult {
                score: 0.8,
                knowledge_diversity: 0.8,
                specialization_diversity: 0.8,
                unique_domains: 5,
                node_count: 3,
                status: DiversityStatus::Healthy,
                warnings: vec![],
            },
            centralization: CentralizationResult {
                gini: 0.2,
                knowledge_gini: 0.2,
                load_gini: 0.2,
                expertise_gini: 0.2,
                status: CentralizationStatus::Decentralized,
                risk_level: RiskLevel::Low,
                recommendations: vec![],
            },
            cascade_risk: CascadeRiskAssessment {
                overall_risk: 0.2,
                high_risk_nodes: vec![],
                cycles: vec![],
                single_points_of_failure: vec![],
                worst_case_scenario: None,
                recommendations: vec![],
            },
            aggregation_quality: None,
            overall_health: 0.8,
            status: OverallHealthStatus::Healthy,
            checked_at: chrono::Utc::now(),
            recommendations: vec![],
        };

        assert!(health.is_healthy());
        assert!(!health.is_at_risk());
    }
}
