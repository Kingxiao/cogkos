//! Model comparison utilities

use super::*;
use serde::{Deserialize, Serialize};

/// Model comparison result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelComparison {
    pub model_a: String,
    pub model_b: String,
    pub accuracy_diff: f64,
    pub latency_diff_ms: i64,
    /// Statistical significance p-value
    pub statistical_significance: Option<f64>,
    pub winner: Option<String>,
}

/// Compare two models
pub fn compare_models(
    model_a_performance: &[ModelPerformanceRecord],
    model_b_performance: &[ModelPerformanceRecord],
) -> ModelComparison {
    let avg_accuracy_a = if model_a_performance.is_empty() {
        0.0
    } else {
        model_a_performance.iter().map(|p| p.accuracy).sum::<f64>()
            / model_a_performance.len() as f64
    };

    let avg_accuracy_b = if model_b_performance.is_empty() {
        0.0
    } else {
        model_b_performance.iter().map(|p| p.accuracy).sum::<f64>()
            / model_b_performance.len() as f64
    };

    let accuracy_diff = avg_accuracy_b - avg_accuracy_a;

    let winner = if accuracy_diff.abs() < 0.01 {
        None
    } else if accuracy_diff > 0.0 {
        model_b_performance.first().map(|p| p.model_id.clone())
    } else {
        model_a_performance.first().map(|p| p.model_id.clone())
    };

    ModelComparison {
        model_a: model_a_performance
            .first()
            .map(|p| p.model_id.clone())
            .unwrap_or_default(),
        model_b: model_b_performance
            .first()
            .map(|p| p.model_id.clone())
            .unwrap_or_default(),
        accuracy_diff,
        latency_diff_ms: 0,
        statistical_significance: None,
        winner,
    }
}
