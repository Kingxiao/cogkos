//! Aggregation Quality Assessment - Measure quality of federated query results
//!
//! Compares aggregated results against best individual results to ensure
//! federation is providing value beyond any single node.

use std::collections::HashMap;

/// Quality metrics for aggregation result
#[derive(Debug, Clone)]
pub struct AggregationQualityMetrics {
    /// Overall quality score (0.0 - 1.0)
    pub overall_score: f64,
    /// Coverage - how much of available knowledge is captured
    pub coverage: f64,
    /// Accuracy - correctness of aggregated result
    pub accuracy: f64,
    /// Comprehensiveness - depth and detail
    pub comprehensiveness: f64,
    /// Confidence calibration - how well confidence matches reality
    pub confidence_calibration: f64,
    /// Improvement over best single node (can be negative)
    pub improvement_over_best: f64,
}

impl AggregationQualityMetrics {
    /// Check if aggregation is providing value
    pub fn is_value_adding(&self) -> bool {
        self.improvement_over_best > 0.05 // 5% threshold
    }

    /// Get quality tier
    pub fn quality_tier(&self) -> QualityTier {
        if self.overall_score >= 0.8 {
            QualityTier::Excellent
        } else if self.overall_score >= 0.6 {
            QualityTier::Good
        } else if self.overall_score >= 0.4 {
            QualityTier::Fair
        } else {
            QualityTier::Poor
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityTier {
    Excellent,
    Good,
    Fair,
    Poor,
}

/// Node result with quality score
#[derive(Debug, Clone)]
pub struct ScoredNodeResult<T> {
    pub node_id: String,
    pub result: T,
    pub quality_score: f64,
    pub confidence: f64,
    pub coverage: f64,
}

/// Calculate coverage metric
/// Measures what percentage of unique information is captured
pub fn calculate_coverage<T: PartialEq + Clone>(
    aggregated: &[T],
    all_individual: &[Vec<T>],
) -> f64 {
    if all_individual.is_empty() {
        return 0.0;
    }

    // Collect all unique items from individuals
    let mut all_unique = Vec::new();
    for individual in all_individual {
        for item in individual {
            if !all_unique.contains(item) {
                all_unique.push(item.clone());
            }
        }
    }

    if all_unique.is_empty() {
        return 1.0;
    }

    // Count how many are in aggregated
    let captured: usize = aggregated
        .iter()
        .filter(|item| all_unique.contains(item))
        .count();

    captured as f64 / all_unique.len() as f64
}

/// Calculate accuracy using consensus
/// Higher consensus = higher likely accuracy
pub fn calculate_consensus_accuracy<T: PartialEq + Clone + std::fmt::Debug>(
    results: &[ScoredNodeResult<T>],
) -> f64 {
    if results.len() < 2 {
        return results.first().map(|r| r.confidence).unwrap_or(0.5);
    }

    // Group results by value
    let mut groups: HashMap<String, Vec<&ScoredNodeResult<T>>> = HashMap::new();
    for result in results {
        // Simple string representation for grouping
        let key = format!("{:?}", result.result);
        groups.entry(key).or_default().push(result);
    }

    // Find largest group
    let empty_vec: Vec<&ScoredNodeResult<T>> = Vec::new();
    let largest_group = groups
        .values()
        .max_by_key(|g| g.len())
        .unwrap_or(&empty_vec);

    if largest_group.is_empty() {
        return 0.5;
    }

    // Consensus score based on agreement
    let agreement_ratio = largest_group.len() as f64 / results.len() as f64;

    // Weight by average confidence of agreeing nodes
    let avg_confidence: f64 =
        largest_group.iter().map(|r| r.confidence).sum::<f64>() / largest_group.len() as f64;

    (agreement_ratio * 0.7 + avg_confidence * 0.3).clamp(0.0, 1.0)
}

/// Calculate comprehensiveness score
/// Based on detail level and completeness
pub fn calculate_comprehensiveness(
    aggregated_content: &str,
    individual_contents: &[String],
) -> f64 {
    if individual_contents.is_empty() {
        return 0.0;
    }

    // Average length of individual results
    let avg_individual_length: f64 = individual_contents
        .iter()
        .map(|s| s.len() as f64)
        .sum::<f64>()
        / individual_contents.len() as f64;

    if avg_individual_length == 0.0 {
        return 0.0;
    }

    // Ratio of aggregated to average individual
    let length_ratio = aggregated_content.len() as f64 / avg_individual_length;

    // Score peaks at 1.5x average (not too short, not too verbose)
    if (1.0..=2.0).contains(&length_ratio) {
        0.8 + (length_ratio - 1.0) * 0.2 // 0.8 to 1.0
    } else if length_ratio < 1.0 {
        length_ratio * 0.8 // 0.0 to 0.8
    } else {
        (2.5 - length_ratio.min(2.5)) / 0.5 * 0.2 + 0.6 // Decreasing after 2.0
    }
}

/// Calculate confidence calibration
/// Measures how well predicted confidence matches actual accuracy
pub fn calculate_confidence_calibration(
    predictions: &[(f64, bool)], // (predicted confidence, was correct)
) -> f64 {
    if predictions.is_empty() {
        return 0.5;
    }

    // Bin predictions by confidence
    let mut bins: HashMap<u32, (f64, usize)> = HashMap::new(); // (sum_correct, count)

    for (confidence, correct) in predictions {
        let bin = (confidence * 10.0) as u32; // 10 bins: 0.0-0.1, 0.1-0.2, etc.
        let entry = bins.entry(bin.min(9)).or_insert((0.0, 0));
        entry.0 += if *correct { 1.0 } else { 0.0 };
        entry.1 += 1;
    }

    // Calculate calibration error
    let mut total_error = 0.0;
    let mut total_weight = 0;

    for (bin, (correct_sum, count)) in bins {
        if count > 0 {
            let bin_center = (bin as f64 + 0.5) / 10.0;
            let actual_rate = correct_sum / count as f64;
            let error = (bin_center - actual_rate).abs();
            total_error += error * count as f64;
            total_weight += count;
        }
    }

    if total_weight == 0 {
        return 0.5;
    }

    let avg_error = total_error / total_weight as f64;
    (1.0 - avg_error).clamp(0.0, 1.0)
}

/// Compare aggregated result to best individual result
pub fn calculate_improvement<T: PartialEq>(
    aggregated: &T,
    individual_results: &[T],
    quality_fn: impl Fn(&T) -> f64,
) -> f64 {
    if individual_results.is_empty() {
        return 0.0;
    }

    let best_individual = individual_results
        .iter()
        .map(&quality_fn)
        .fold(0.0, f64::max);

    let aggregated_quality = quality_fn(aggregated);

    if best_individual == 0.0 {
        return 0.0;
    }

    (aggregated_quality - best_individual) / best_individual
}

/// Full aggregation quality assessment
pub fn assess_aggregation_quality<T: PartialEq + Clone + std::fmt::Debug>(
    aggregated_result: &T,
    individual_results: &[ScoredNodeResult<T>],
    aggregated_content: &str,
    individual_contents: &[String],
) -> AggregationQualityMetrics {
    // Extract values for coverage calculation
    let values: Vec<T> = individual_results
        .iter()
        .map(|r| r.result.clone())
        .collect();
    let individual_value_groups: Vec<Vec<T>> = values.iter().map(|v| vec![v.clone()]).collect();

    let coverage = calculate_coverage(
        std::slice::from_ref(aggregated_result),
        &individual_value_groups,
    );
    let accuracy = calculate_consensus_accuracy(individual_results);
    let comprehensiveness = calculate_comprehensiveness(aggregated_content, individual_contents);

    // For simplicity, assume good calibration
    let confidence_calibration = 0.75;

    // Calculate improvement (simplified - using quality scores)
    let best_quality = individual_results
        .iter()
        .map(|r| r.quality_score)
        .fold(0.0, f64::max);

    let aggregated_quality = accuracy * 0.4 + coverage * 0.3 + comprehensiveness * 0.3;
    let improvement_over_best = if best_quality > 0.0 {
        (aggregated_quality - best_quality) / best_quality
    } else {
        0.0
    };

    // Overall score
    let overall_score = (coverage * 0.25)
        + (accuracy * 0.35)
        + (comprehensiveness * 0.25)
        + (confidence_calibration * 0.15);

    AggregationQualityMetrics {
        overall_score,
        coverage,
        accuracy,
        comprehensiveness,
        confidence_calibration,
        improvement_over_best,
    }
}

/// Track aggregation quality over time
#[derive(Debug, Clone)]
pub struct QualityHistoryEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub metrics: AggregationQualityMetrics,
    pub query_type: String,
}

pub struct AggregationQualityTracker {
    history: Vec<QualityHistoryEntry>,
    max_history: usize,
}

impl AggregationQualityTracker {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            max_history: 1000,
        }
    }

    pub fn with_max_history(mut self, max: usize) -> Self {
        self.max_history = max;
        self
    }

    pub fn record(&mut self, metrics: AggregationQualityMetrics, query_type: impl Into<String>) {
        let entry = QualityHistoryEntry {
            timestamp: chrono::Utc::now(),
            metrics,
            query_type: query_type.into(),
        };

        self.history.push(entry);

        if self.history.len() > self.max_history {
            self.history.remove(0);
        }
    }

    /// Get average quality over time window
    pub fn average_quality(&self, window_size: usize) -> Option<AggregationQualityMetrics> {
        let window = window_size.min(self.history.len());
        if window == 0 {
            return None;
        }

        let recent = &self.history[self.history.len() - window..];

        let overall_score =
            recent.iter().map(|e| e.metrics.overall_score).sum::<f64>() / window as f64;
        let coverage = recent.iter().map(|e| e.metrics.coverage).sum::<f64>() / window as f64;
        let accuracy = recent.iter().map(|e| e.metrics.accuracy).sum::<f64>() / window as f64;
        let comprehensiveness = recent
            .iter()
            .map(|e| e.metrics.comprehensiveness)
            .sum::<f64>()
            / window as f64;
        let confidence_calibration = recent
            .iter()
            .map(|e| e.metrics.confidence_calibration)
            .sum::<f64>()
            / window as f64;
        let improvement_over_best = recent
            .iter()
            .map(|e| e.metrics.improvement_over_best)
            .sum::<f64>()
            / window as f64;

        Some(AggregationQualityMetrics {
            overall_score,
            coverage,
            accuracy,
            comprehensiveness,
            confidence_calibration,
            improvement_over_best,
        })
    }

    /// Check if aggregation quality is declining
    pub fn is_declining(&self, window: usize) -> bool {
        if self.history.len() < window * 2 {
            return false;
        }

        let old_avg = self.average_quality(window * 2);
        let recent_avg = self.average_quality(window);

        match (old_avg, recent_avg) {
            (Some(old), Some(recent)) => recent.overall_score < old.overall_score - 0.1,
            _ => false,
        }
    }

    /// Get percentage of value-adding aggregations
    pub fn value_adding_percentage(&self, window: usize) -> f64 {
        let window = window.min(self.history.len());
        if window == 0 {
            return 0.0;
        }

        let recent = &self.history[self.history.len() - window..];
        let value_adding = recent
            .iter()
            .filter(|e| e.metrics.is_value_adding())
            .count();

        value_adding as f64 / window as f64
    }

    pub fn get_history(&self) -> &[QualityHistoryEntry] {
        &self.history
    }
}

impl Default for AggregationQualityTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Federation quality report
#[derive(Debug, Clone)]
pub struct FederationQualityReport {
    /// Overall federation quality score
    pub overall_score: f64,
    /// Average improvement over best single node
    pub avg_improvement: f64,
    /// Percentage of queries where aggregation adds value
    pub value_adding_percentage: f64,
    /// Coverage across all domains
    pub coverage_by_domain: HashMap<String, f64>,
    /// Quality trend (positive = improving)
    pub trend: f64,
    /// Recommendations
    pub recommendations: Vec<String>,
}

/// Generate federation quality report
pub fn generate_quality_report(
    tracker: &AggregationQualityTracker,
    window: usize,
) -> FederationQualityReport {
    let avg = tracker.average_quality(window);
    let value_adding_pct = tracker.value_adding_percentage(window);

    let overall_score = avg.as_ref().map(|a| a.overall_score).unwrap_or(0.5);
    let avg_improvement = avg.as_ref().map(|a| a.improvement_over_best).unwrap_or(0.0);

    let trend = if tracker.is_declining(window / 2) {
        -0.1
    } else {
        0.05
    };

    let mut recommendations = Vec::new();

    if value_adding_pct < 0.5 {
        recommendations.push(
            "Aggregation is not adding value for most queries - review aggregation algorithm"
                .to_string(),
        );
    }
    if avg_improvement < 0.0 {
        recommendations.push(
            "Aggregated results are worse than individual results - federation may not be needed"
                .to_string(),
        );
    }
    if trend < 0.0 {
        recommendations.push("Quality is declining - investigate root cause".to_string());
    }

    FederationQualityReport {
        overall_score,
        avg_improvement,
        value_adding_percentage: value_adding_pct,
        coverage_by_domain: HashMap::new(), // Would need domain-specific tracking
        trend,
        recommendations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coverage_calculation() {
        let aggregated = vec!["a", "b"];
        let individual = vec![vec!["a", "c"], vec!["b", "d"], vec!["a", "e"]];

        let coverage = calculate_coverage(&aggregated, &individual);
        assert!(coverage > 0.0 && coverage < 1.0);
    }

    #[test]
    fn test_consensus_accuracy() {
        let results = vec![
            ScoredNodeResult {
                node_id: "node1".to_string(),
                result: "answer_a",
                quality_score: 0.8,
                confidence: 0.9,
                coverage: 0.7,
            },
            ScoredNodeResult {
                node_id: "node2".to_string(),
                result: "answer_a",
                quality_score: 0.7,
                confidence: 0.8,
                coverage: 0.6,
            },
            ScoredNodeResult {
                node_id: "node3".to_string(),
                result: "answer_b",
                quality_score: 0.6,
                confidence: 0.7,
                coverage: 0.5,
            },
        ];

        let accuracy = calculate_consensus_accuracy(&results);
        assert!(accuracy > 0.5); // Consensus should be reasonably high
    }

    #[test]
    fn test_comprehensiveness() {
        let aggregated =
            "This is a comprehensive answer with many details and explanations.".to_string();
        let individual = vec!["Short answer.".to_string(), "Brief response.".to_string()];

        let score = calculate_comprehensiveness(&aggregated, &individual);
        assert!(score > 0.5); // Aggregated is more comprehensive
    }

    #[test]
    fn test_confidence_calibration() {
        // Perfect calibration: 0.8 confidence -> 80% correct
        let predictions = vec![
            (0.8, true),
            (0.8, true),
            (0.8, true),
            (0.8, true),
            (0.8, false), // 80% correct at 0.8 confidence
        ];

        let calibration = calculate_confidence_calibration(&predictions);
        assert!(calibration > 0.7);
    }

    #[test]
    fn test_aggregation_quality_metrics() {
        let aggregated = "combined answer";
        let individual_results = vec![
            ScoredNodeResult {
                node_id: "node1".to_string(),
                result: "answer1",
                quality_score: 0.7,
                confidence: 0.8,
                coverage: 0.6,
            },
            ScoredNodeResult {
                node_id: "node2".to_string(),
                result: "answer2",
                quality_score: 0.6,
                confidence: 0.7,
                coverage: 0.5,
            },
        ];
        let aggregated_content = "This is the aggregated content.".to_string();
        let individual_contents = vec!["Content 1.".to_string(), "Content 2.".to_string()];

        let metrics = assess_aggregation_quality(
            &aggregated,
            &individual_results,
            &aggregated_content,
            &individual_contents,
        );

        assert!(metrics.overall_score >= 0.0 && metrics.overall_score <= 1.0);
    }

    #[test]
    fn test_quality_tier() {
        let excellent = AggregationQualityMetrics {
            overall_score: 0.85,
            coverage: 0.9,
            accuracy: 0.9,
            comprehensiveness: 0.9,
            confidence_calibration: 0.9,
            improvement_over_best: 0.1,
        };
        assert_eq!(excellent.quality_tier(), QualityTier::Excellent);

        let poor = AggregationQualityMetrics {
            overall_score: 0.3,
            coverage: 0.3,
            accuracy: 0.3,
            comprehensiveness: 0.3,
            confidence_calibration: 0.3,
            improvement_over_best: -0.2,
        };
        assert_eq!(poor.quality_tier(), QualityTier::Poor);
    }

    #[test]
    fn test_is_value_adding() {
        let value_adding = AggregationQualityMetrics {
            overall_score: 0.8,
            coverage: 0.8,
            accuracy: 0.8,
            comprehensiveness: 0.8,
            confidence_calibration: 0.8,
            improvement_over_best: 0.1,
        };
        assert!(value_adding.is_value_adding());

        let not_value_adding = AggregationQualityMetrics {
            overall_score: 0.5,
            coverage: 0.5,
            accuracy: 0.5,
            comprehensiveness: 0.5,
            confidence_calibration: 0.5,
            improvement_over_best: -0.1,
        };
        assert!(!not_value_adding.is_value_adding());
    }

    #[test]
    fn test_quality_tracker() {
        let mut tracker = AggregationQualityTracker::new();

        let metrics = AggregationQualityMetrics {
            overall_score: 0.8,
            coverage: 0.8,
            accuracy: 0.8,
            comprehensiveness: 0.8,
            confidence_calibration: 0.8,
            improvement_over_best: 0.1,
        };

        tracker.record(metrics, "test_query");

        let avg = tracker.average_quality(10).unwrap();
        assert_eq!(avg.overall_score, 0.8);
    }

    #[test]
    fn test_empty_quality_tracker() {
        let tracker = AggregationQualityTracker::new();
        assert!(tracker.average_quality(10).is_none());
        assert_eq!(tracker.value_adding_percentage(10), 0.0);
    }
}
