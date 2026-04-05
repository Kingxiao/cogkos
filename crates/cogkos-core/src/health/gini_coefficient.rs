//! Gini Coefficient - Measure concentration/inequality in federation
//!
//! The Gini coefficient measures statistical dispersion intended to represent
//! the income or wealth inequality within a nation or any other group of people.
//! Here we use it to measure knowledge concentration across nodes.
//!
//! Range: 0.0 (perfect equality) to 1.0 (perfect inequality)
//! For federation health: Lower is better (more equal distribution)

/// Calculate Gini coefficient from a slice of values
///
/// # Arguments
/// * `values` - Slice of f64 values representing distribution (e.g., query loads, knowledge counts)
///
/// # Returns
/// Gini coefficient (0.0 = perfect equality, 1.0 = perfect inequality)
pub fn calculate_gini_coefficient(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }

    let n = values.len() as f64;
    let sum: f64 = values.iter().sum();

    if sum == 0.0 {
        return 0.0;
    }

    // Sort values for calculation
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    // Calculate Gini using the formula: G = (2 * sum(i * y_i)) / (n * sum(y_i)) - (n + 1) / n
    let mut weighted_sum = 0.0;

    for (i, &value) in sorted.iter().enumerate() {
        weighted_sum += (i as f64 + 1.0) * value;
    }

    let gini = (2.0 * weighted_sum) / (n * sum) - (n + 1.0) / n;

    gini.clamp(0.0, 1.0)
}

/// Calculate Gini coefficient for knowledge distribution
pub fn calculate_knowledge_gini(knowledge_counts: &[usize]) -> f64 {
    let values: Vec<f64> = knowledge_counts.iter().map(|&c| c as f64).collect();
    calculate_gini_coefficient(&values)
}

/// Calculate Gini coefficient for query load distribution
pub fn calculate_load_gini(query_counts: &[u64]) -> f64 {
    let values: Vec<f64> = query_counts.iter().map(|&c| c as f64).collect();
    calculate_gini_coefficient(&values)
}

/// Calculate Gini coefficient for expertise scores
pub fn calculate_expertise_gini(expertise_scores: &[f64]) -> f64 {
    calculate_gini_coefficient(expertise_scores)
}

/// Centralization detection result
#[derive(Debug, Clone, PartialEq)]
pub struct CentralizationResult {
    /// Overall Gini coefficient
    pub gini: f64,
    /// Knowledge distribution Gini
    pub knowledge_gini: f64,
    /// Load distribution Gini
    pub load_gini: f64,
    /// Expertise concentration Gini
    pub expertise_gini: f64,
    /// Centralization status
    pub status: CentralizationStatus,
    /// Risk assessment
    pub risk_level: RiskLevel,
    /// Recommendations
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CentralizationStatus {
    Decentralized,
    Moderate,
    Centralized,
    HighlyCentralized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl CentralizationResult {
    /// Check if the federation is at risk due to centralization
    pub fn is_at_risk(&self) -> bool {
        matches!(self.risk_level, RiskLevel::High | RiskLevel::Critical)
    }

    /// Check if immediate action is needed
    pub fn needs_action(&self) -> bool {
        self.risk_level == RiskLevel::Critical
    }
}

/// Comprehensive centralization analysis
pub fn analyze_centralization(
    knowledge_per_node: &[usize],
    queries_per_node: &[u64],
    expertise_scores: &[f64],
) -> CentralizationResult {
    let knowledge_gini = calculate_knowledge_gini(knowledge_per_node);
    let load_gini = calculate_load_gini(queries_per_node);
    let expertise_gini = calculate_expertise_gini(expertise_scores);

    // Weighted average
    let gini = (knowledge_gini * 0.4) + (load_gini * 0.4) + (expertise_gini * 0.2);

    // Determine status
    let status = if gini < 0.2 {
        CentralizationStatus::Decentralized
    } else if gini < 0.4 {
        CentralizationStatus::Moderate
    } else if gini < 0.6 {
        CentralizationStatus::Centralized
    } else {
        CentralizationStatus::HighlyCentralized
    };

    // Determine risk level
    let risk_level = if gini < 0.3 {
        RiskLevel::Low
    } else if gini < 0.5 {
        RiskLevel::Medium
    } else if gini < 0.7 {
        RiskLevel::High
    } else {
        RiskLevel::Critical
    };

    // Generate recommendations
    let mut recommendations = Vec::new();

    if knowledge_gini > 0.5 {
        recommendations
            .push("Knowledge is too concentrated - redistribute knowledge base".to_string());
    }
    if load_gini > 0.5 {
        recommendations
            .push("Query load is unbalanced - implement better load balancing".to_string());
    }
    if expertise_gini > 0.6 {
        recommendations.push("Expertise is too centralized - cross-train nodes".to_string());
    }
    if gini > 0.7 {
        recommendations.push(
            "CRITICAL: Federation is highly centralized - immediate action required".to_string(),
        );
    }

    CentralizationResult {
        gini,
        knowledge_gini,
        load_gini,
        expertise_gini,
        status,
        risk_level,
        recommendations,
    }
}

/// Node contribution analysis - identify over/under-contributing nodes
#[derive(Debug, Clone)]
pub struct NodeContribution {
    pub node_id: String,
    pub knowledge_share: f64,     // Percentage of total knowledge
    pub query_share: f64,         // Percentage of total queries handled
    pub contribution_score: f64,  // Combined score
    pub deviation_from_mean: f64, // How much above/below average
}

pub fn analyze_node_contributions(
    node_ids: &[String],
    knowledge_counts: &[usize],
    query_counts: &[u64],
) -> Vec<NodeContribution> {
    assert_eq!(node_ids.len(), knowledge_counts.len());
    assert_eq!(node_ids.len(), query_counts.len());

    let total_knowledge: usize = knowledge_counts.iter().sum();
    let total_queries: u64 = query_counts.iter().sum();
    let n = node_ids.len() as f64;

    let mean_knowledge_share = if total_knowledge > 0 { 1.0 / n } else { 0.0 };
    let mean_query_share = if total_queries > 0 { 1.0 / n } else { 0.0 };

    node_ids
        .iter()
        .enumerate()
        .map(|(i, node_id)| {
            let knowledge_share = if total_knowledge > 0 {
                knowledge_counts[i] as f64 / total_knowledge as f64
            } else {
                0.0
            };

            let query_share = if total_queries > 0 {
                query_counts[i] as f64 / total_queries as f64
            } else {
                0.0
            };

            let contribution_score = (knowledge_share + query_share) / 2.0;
            let deviation_from_mean =
                ((knowledge_share - mean_knowledge_share) + (query_share - mean_query_share)) / 2.0;

            NodeContribution {
                node_id: node_id.clone(),
                knowledge_share,
                query_share,
                contribution_score,
                deviation_from_mean,
            }
        })
        .collect()
}

/// Identify dominant nodes (potential single points of failure)
pub fn identify_dominant_nodes(contributions: &[NodeContribution], threshold: f64) -> Vec<String> {
    contributions
        .iter()
        .filter(|c| c.contribution_score > threshold)
        .map(|c| c.node_id.clone())
        .collect()
}

/// Lorenz curve calculation for visualization
/// Returns cumulative population share and cumulative value share
pub fn calculate_lorenz_curve(values: &[f64]) -> Vec<(f64, f64)> {
    if values.is_empty() {
        return vec![(0.0, 0.0), (1.0, 1.0)];
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let total: f64 = sorted.iter().sum();
    let n = sorted.len() as f64;

    let mut curve = vec![(0.0, 0.0)];
    let mut cumsum = 0.0;

    for (i, &value) in sorted.iter().enumerate() {
        cumsum += value;
        let pop_share = (i as f64 + 1.0) / n;
        let value_share = if total > 0.0 { cumsum / total } else { 0.0 };
        curve.push((pop_share, value_share));
    }

    curve
}

/// Time-series tracking of Gini coefficient
#[derive(Debug, Clone)]
pub struct GiniHistoryEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub gini: f64,
    pub knowledge_gini: f64,
    pub load_gini: f64,
}

pub struct GiniTracker {
    history: Vec<GiniHistoryEntry>,
    max_history: usize,
}

impl GiniTracker {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            max_history: 100,
        }
    }

    pub fn with_max_history(mut self, max: usize) -> Self {
        self.max_history = max;
        self
    }

    pub fn record(&mut self, result: &CentralizationResult) {
        let entry = GiniHistoryEntry {
            timestamp: chrono::Utc::now(),
            gini: result.gini,
            knowledge_gini: result.knowledge_gini,
            load_gini: result.load_gini,
        };

        self.history.push(entry);

        if self.history.len() > self.max_history {
            self.history.remove(0);
        }
    }

    /// Check if centralization is trending worse
    pub fn is_worsening(&self, window: usize) -> bool {
        if self.history.len() < window * 2 {
            return false;
        }

        let recent: Vec<f64> = self
            .history
            .iter()
            .rev()
            .take(window)
            .map(|h| h.gini)
            .collect();
        let previous: Vec<f64> = self
            .history
            .iter()
            .rev()
            .skip(window)
            .take(window)
            .map(|h| h.gini)
            .collect();

        let recent_avg: f64 = recent.iter().sum::<f64>() / recent.len() as f64;
        let previous_avg: f64 = previous.iter().sum::<f64>() / previous.len() as f64;

        recent_avg > previous_avg + 0.05 // Threshold for significant worsening
    }

    pub fn get_history(&self) -> &[GiniHistoryEntry] {
        &self.history
    }
}

impl Default for GiniTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gini_perfect_equality() {
        // All values equal
        let values = vec![10.0, 10.0, 10.0, 10.0];
        let gini = calculate_gini_coefficient(&values);
        assert!((gini - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_gini_perfect_inequality() {
        // One node has everything
        let values = vec![0.0, 0.0, 0.0, 100.0];
        let gini = calculate_gini_coefficient(&values);
        assert!(gini > 0.7);
    }

    #[test]
    fn test_gini_moderate_inequality() {
        // Some inequality
        let values = vec![10.0, 20.0, 30.0, 40.0];
        let gini = calculate_gini_coefficient(&values);
        assert!(gini > 0.1 && gini < 0.5);
    }

    #[test]
    fn test_gini_single_value() {
        let values = vec![100.0];
        let gini = calculate_gini_coefficient(&values);
        assert_eq!(gini, 0.0);
    }

    #[test]
    fn test_gini_empty() {
        let values: Vec<f64> = vec![];
        let gini = calculate_gini_coefficient(&values);
        assert_eq!(gini, 0.0);
    }

    #[test]
    fn test_analyze_centralization() {
        let knowledge = vec![100, 100, 100, 100]; // Equal
        let queries = vec![1000, 1000, 1000, 1000]; // Equal
        let expertise = vec![0.5, 0.5, 0.5, 0.5]; // Equal

        let result = analyze_centralization(&knowledge, &queries, &expertise);

        assert!(result.gini < 0.2);
        assert_eq!(result.status, CentralizationStatus::Decentralized);
        assert_eq!(result.risk_level, RiskLevel::Low);
    }

    #[test]
    fn test_analyze_centralization_high() {
        let knowledge = vec![1, 1, 1, 997]; // Highly concentrated
        let queries = vec![10, 10, 10, 970];
        let expertise = vec![0.1, 0.1, 0.1, 0.7];

        let result = analyze_centralization(&knowledge, &queries, &expertise);

        assert!(result.gini > 0.5);
        assert_eq!(result.status, CentralizationStatus::HighlyCentralized);
        assert!(result.risk_level == RiskLevel::High || result.risk_level == RiskLevel::Critical);
        assert!(!result.recommendations.is_empty());
    }

    #[test]
    fn test_node_contributions() {
        let node_ids = vec![
            "node1".to_string(),
            "node2".to_string(),
            "node3".to_string(),
            "node4".to_string(),
        ];
        let knowledge = vec![25, 25, 25, 25];
        let queries = vec![250, 250, 250, 250];

        let contributions = analyze_node_contributions(&node_ids, &knowledge, &queries);

        assert_eq!(contributions.len(), 4);
        assert!((contributions[0].knowledge_share - 0.25).abs() < 0.01);
        assert!((contributions[0].query_share - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_identify_dominant_nodes() {
        let contributions = vec![
            NodeContribution {
                node_id: "node1".to_string(),
                knowledge_share: 0.1,
                query_share: 0.1,
                contribution_score: 0.1,
                deviation_from_mean: -0.15,
            },
            NodeContribution {
                node_id: "node2".to_string(),
                knowledge_share: 0.6,
                query_share: 0.6,
                contribution_score: 0.6,
                deviation_from_mean: 0.35,
            },
        ];

        let dominant = identify_dominant_nodes(&contributions, 0.5);

        assert_eq!(dominant.len(), 1);
        assert_eq!(dominant[0], "node2");
    }

    #[test]
    fn test_lorenz_curve() {
        let values = vec![10.0, 20.0, 30.0, 40.0];
        let curve = calculate_lorenz_curve(&values);

        assert_eq!(curve[0], (0.0, 0.0));
        assert_eq!(curve[curve.len() - 1], (1.0, 1.0));
    }

    #[test]
    fn test_gini_tracker() {
        let mut tracker = GiniTracker::new();

        let result = CentralizationResult {
            gini: 0.3,
            knowledge_gini: 0.25,
            load_gini: 0.35,
            expertise_gini: 0.2,
            status: CentralizationStatus::Moderate,
            risk_level: RiskLevel::Low,
            recommendations: vec![],
        };

        tracker.record(&result);

        assert_eq!(tracker.get_history().len(), 1);
        assert_eq!(tracker.get_history()[0].gini, 0.3);
    }

    #[test]
    fn test_gini_is_worsening() {
        let mut tracker = GiniTracker::new();

        // Add entries showing worsening trend
        for i in 0..10 {
            let result = CentralizationResult {
                gini: 0.2 + (i as f64 * 0.05),
                knowledge_gini: 0.2,
                load_gini: 0.2,
                expertise_gini: 0.2,
                status: CentralizationStatus::Moderate,
                risk_level: RiskLevel::Low,
                recommendations: vec![],
            };
            tracker.record(&result);
        }

        assert!(tracker.is_worsening(3));
    }
}
