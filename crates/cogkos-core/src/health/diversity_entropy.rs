//! Diversity Entropy - Measure knowledge diversity across federation nodes
//!
//! Shannon entropy is used to quantify the diversity of knowledge sources.
//! Higher entropy indicates more diverse knowledge (good for federation).
//! Lower entropy indicates concentration risk.

use std::collections::HashMap;

/// Shannon entropy calculation for diversity measurement
pub fn calculate_shannon_entropy(probabilities: &[f64]) -> f64 {
    probabilities
        .iter()
        .filter(|&&p| p > 0.0)
        .map(|&p| -p * p.log2())
        .sum()
}

/// Calculate normalized entropy (0.0 to 1.0)
/// Higher is better (more diverse)
pub fn calculate_normalized_entropy(probabilities: &[f64]) -> f64 {
    if probabilities.is_empty() {
        return 0.0;
    }

    let entropy = calculate_shannon_entropy(probabilities);
    let max_entropy = (probabilities.len() as f64).log2();

    if max_entropy == 0.0 {
        0.0
    } else {
        (entropy / max_entropy).clamp(0.0, 1.0)
    }
}

/// Calculate knowledge domain diversity across federation nodes
///
/// # Arguments
/// * `node_domains` - Map of node_id to list of domains it covers
///
/// # Returns
/// * Diversity score (0.0 - 1.0), where higher means more diverse
pub fn calculate_knowledge_diversity(node_domains: &HashMap<String, Vec<String>>) -> f64 {
    // Count domain frequencies across all nodes
    let mut domain_counts: HashMap<String, usize> = HashMap::new();
    let mut total_domain_instances = 0usize;

    for domains in node_domains.values() {
        for domain in domains {
            *domain_counts.entry(domain.clone()).or_insert(0) += 1;
            total_domain_instances += 1;
        }
    }

    if total_domain_instances == 0 {
        return 0.0;
    }

    // Calculate probabilities
    let probabilities: Vec<f64> = domain_counts
        .values()
        .map(|&count| count as f64 / total_domain_instances as f64)
        .collect();

    calculate_normalized_entropy(&probabilities)
}

/// Calculate node specialization diversity
/// Measures how evenly distributed specializations are across nodes
pub fn calculate_specialization_diversity(
    node_expertise: &HashMap<String, HashMap<String, f64>>,
) -> f64 {
    if node_expertise.is_empty() {
        return 0.0;
    }

    // Collect all expertise areas
    let mut all_expertise: HashMap<String, Vec<f64>> = HashMap::new();

    for expertise_map in node_expertise.values() {
        for (domain, score) in expertise_map {
            all_expertise
                .entry(domain.clone())
                .or_default()
                .push(*score);
        }
    }

    // Calculate distribution of domains per node
    let domains_per_node: Vec<f64> = node_expertise
        .values()
        .map(|expertise| expertise.len() as f64)
        .collect();

    let total: f64 = domains_per_node.iter().sum();
    if total == 0.0 {
        return 0.0;
    }

    let probabilities: Vec<f64> = domains_per_node
        .iter()
        .map(|&count| count / total)
        .collect();

    calculate_normalized_entropy(&probabilities)
}

/// Diversity health check result
#[derive(Debug, Clone, PartialEq)]
pub struct DiversityHealthResult {
    /// Overall diversity score (0.0 - 1.0)
    pub score: f64,
    /// Knowledge domain diversity
    pub knowledge_diversity: f64,
    /// Specialization distribution diversity
    pub specialization_diversity: f64,
    /// Number of unique domains
    pub unique_domains: usize,
    /// Number of nodes
    pub node_count: usize,
    /// Health status
    pub status: DiversityStatus,
    /// Warnings if any
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiversityStatus {
    Healthy,
    Warning,
    Critical,
}

impl DiversityHealthResult {
    /// Check if diversity is healthy
    pub fn is_healthy(&self) -> bool {
        self.status == DiversityStatus::Healthy
    }
}

/// Calculate complete diversity health check
///
/// # Arguments
/// * `node_domains` - Map of node_id to domains it covers
/// * `node_expertise` - Map of node_id to domain expertise scores
///
/// # Returns
/// Complete diversity health check result
pub fn calculate_diversity_health(
    node_domains: &HashMap<String, Vec<String>>,
    node_expertise: &HashMap<String, HashMap<String, f64>>,
) -> DiversityHealthResult {
    let knowledge_diversity = calculate_knowledge_diversity(node_domains);
    let specialization_diversity = calculate_specialization_diversity(node_expertise);

    // Calculate unique domains
    let mut unique_domains = std::collections::HashSet::new();
    for domains in node_domains.values() {
        for domain in domains {
            unique_domains.insert(domain.clone());
        }
    }

    // Calculate weighted overall score
    let score = (knowledge_diversity * 0.6) + (specialization_diversity * 0.4);

    // Determine status
    let status = if score > 0.7 {
        DiversityStatus::Healthy
    } else if score > 0.4 {
        DiversityStatus::Warning
    } else {
        DiversityStatus::Critical
    };

    // Generate warnings
    let mut warnings = Vec::new();
    if knowledge_diversity < 0.5 {
        warnings.push(
            "Low knowledge diversity - consider adding nodes with different domains".to_string(),
        );
    }
    if specialization_diversity < 0.5 {
        warnings
            .push("Uneven specialization distribution - some nodes may be overloaded".to_string());
    }
    if unique_domains.len() < 3 {
        warnings.push("Too few unique domains for robust federation".to_string());
    }

    DiversityHealthResult {
        score,
        knowledge_diversity,
        specialization_diversity,
        unique_domains: unique_domains.len(),
        node_count: node_domains.len(),
        status,
        warnings,
    }
}

/// Temporal diversity analysis - track diversity changes over time
#[derive(Debug, Clone)]
pub struct DiversityHistory {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub score: f64,
    pub knowledge_diversity: f64,
    pub specialization_diversity: f64,
}

pub struct DiversityTracker {
    history: Vec<DiversityHistory>,
    max_history: usize,
}

impl DiversityTracker {
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

    pub fn record(&mut self, result: &DiversityHealthResult) {
        let entry = DiversityHistory {
            timestamp: chrono::Utc::now(),
            score: result.score,
            knowledge_diversity: result.knowledge_diversity,
            specialization_diversity: result.specialization_diversity,
        };

        self.history.push(entry);

        // Keep history within limits
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }
    }

    /// Get diversity trend (positive = improving)
    pub fn trend(&self, window_size: usize) -> f64 {
        if self.history.len() < 2 {
            return 0.0;
        }

        let window = window_size.min(self.history.len());
        let recent: Vec<f64> = self
            .history
            .iter()
            .rev()
            .take(window)
            .map(|h| h.score)
            .collect();

        if recent.len() < 2 {
            return 0.0;
        }

        // Simple linear trend
        let first = recent[recent.len() - 1];
        let last = recent[0];
        last - first
    }

    pub fn get_history(&self) -> &[DiversityHistory] {
        &self.history
    }
}

impl Default for DiversityTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shannon_entropy_uniform() {
        // Uniform distribution should have max entropy
        let probs = vec![0.25, 0.25, 0.25, 0.25];
        let entropy = calculate_normalized_entropy(&probs);
        assert!((entropy - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_shannon_entropy_concentrated() {
        // Concentrated distribution should have low entropy
        let probs = vec![0.9, 0.05, 0.03, 0.02];
        let entropy = calculate_normalized_entropy(&probs);
        assert!(entropy < 0.5);
    }

    #[test]
    fn test_knowledge_diversity() {
        let mut node_domains: HashMap<String, Vec<String>> = HashMap::new();

        // Add nodes with diverse domains
        node_domains.insert(
            "node1".to_string(),
            vec!["tech".to_string(), "science".to_string()],
        );
        node_domains.insert(
            "node2".to_string(),
            vec!["business".to_string(), "finance".to_string()],
        );
        node_domains.insert(
            "node3".to_string(),
            vec!["health".to_string(), "science".to_string()],
        );

        let diversity = calculate_knowledge_diversity(&node_domains);
        assert!(diversity > 0.7); // Should be quite diverse
    }

    #[test]
    fn test_knowledge_diversity_low() {
        let mut node_domains: HashMap<String, Vec<String>> = HashMap::new();

        // All nodes have same domain
        node_domains.insert("node1".to_string(), vec!["tech".to_string()]);
        node_domains.insert("node2".to_string(), vec!["tech".to_string()]);
        node_domains.insert("node3".to_string(), vec!["tech".to_string()]);

        let diversity = calculate_knowledge_diversity(&node_domains);
        assert!(diversity < 0.5);
    }

    #[test]
    fn test_specialization_diversity() {
        let mut node_expertise: HashMap<String, HashMap<String, f64>> = HashMap::new();

        let mut node1_exp = HashMap::new();
        node1_exp.insert("tech".to_string(), 0.9);
        node1_exp.insert("science".to_string(), 0.7);
        node_expertise.insert("node1".to_string(), node1_exp);

        let mut node2_exp = HashMap::new();
        node2_exp.insert("business".to_string(), 0.8);
        node2_exp.insert("finance".to_string(), 0.9);
        node_expertise.insert("node2".to_string(), node2_exp);

        let diversity = calculate_specialization_diversity(&node_expertise);
        assert!(diversity > 0.5);
    }

    #[test]
    fn test_diversity_health_result() {
        let mut node_domains: HashMap<String, Vec<String>> = HashMap::new();
        let mut node_expertise: HashMap<String, HashMap<String, f64>> = HashMap::new();

        // Healthy setup
        node_domains.insert(
            "node1".to_string(),
            vec!["tech".to_string(), "science".to_string()],
        );
        node_domains.insert(
            "node2".to_string(),
            vec!["business".to_string(), "finance".to_string()],
        );

        let mut node1_exp = HashMap::new();
        node1_exp.insert("tech".to_string(), 0.9);
        node_expertise.insert("node1".to_string(), node1_exp);

        let mut node2_exp = HashMap::new();
        node2_exp.insert("business".to_string(), 0.8);
        node_expertise.insert("node2".to_string(), node2_exp);

        let result = calculate_diversity_health(&node_domains, &node_expertise);

        assert!(result.score > 0.0);
        assert_eq!(result.node_count, 2);
        assert_eq!(result.unique_domains, 4);
    }

    #[test]
    fn test_diversity_tracker() {
        let mut tracker = DiversityTracker::new();

        let result = DiversityHealthResult {
            score: 0.8,
            knowledge_diversity: 0.7,
            specialization_diversity: 0.9,
            unique_domains: 5,
            node_count: 3,
            status: DiversityStatus::Healthy,
            warnings: vec![],
        };

        tracker.record(&result);

        assert_eq!(tracker.get_history().len(), 1);
        assert_eq!(tracker.get_history()[0].score, 0.8);
    }

    #[test]
    fn test_empty_diversity() {
        let node_domains: HashMap<String, Vec<String>> = HashMap::new();
        let node_expertise: HashMap<String, HashMap<String, f64>> = HashMap::new();

        let result = calculate_diversity_health(&node_domains, &node_expertise);

        assert_eq!(result.score, 0.0);
        assert_eq!(result.status, DiversityStatus::Critical);
    }
}
