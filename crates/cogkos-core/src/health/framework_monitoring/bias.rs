//! Systematic bias detection

use super::tracker::PredictionTracker;
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;

/// Systematic bias detection
#[derive(Debug, Clone)]
pub struct SystematicBias {
    /// Type of bias
    pub bias_type: BiasType,
    /// Direction of bias
    pub direction: BiasDirection,
    /// Magnitude (0.0 - 1.0)
    pub magnitude: f64,
    /// Affected domains/areas
    pub affected_areas: Vec<String>,
    /// First detected
    pub detected_at: DateTime<Utc>,
    /// Confidence in detection
    pub confidence: f64,
    /// Examples of biased predictions
    pub examples: Vec<BiasExample>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiasType {
    /// Consistently over/under estimating
    CalibrationError,
    /// Bias toward certain outcomes
    OutcomeBias,
    /// Bias in specific contexts
    ContextBias,
    /// Temporal bias (recency/familiarity)
    TemporalBias,
    /// Correlation vs causation confusion
    CorrelationBias,
    /// Availability bias
    AvailabilityBias,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiasDirection {
    Positive, // Overestimation
    Negative, // Underestimation
    Neutral,  // Context-dependent
}

#[derive(Debug, Clone)]
pub struct BiasExample {
    pub timestamp: DateTime<Utc>,
    pub context: String,
    pub prediction: String,
    pub actual: String,
    pub bias_manifestation: String,
}

/// Bias detector
pub struct BiasDetector {
    /// Detected biases
    biases: Vec<SystematicBias>,
    /// Detection configuration
    config: BiasDetectionConfig,
    /// Historical accuracy by category
    category_accuracy: HashMap<String, Vec<(DateTime<Utc>, f64)>>,
}

#[derive(Debug, Clone)]
pub struct BiasDetectionConfig {
    /// Minimum sample size for detection
    pub min_sample_size: usize,
    /// Threshold for flagging bias
    pub bias_threshold: f64,
    /// Window size for trend detection
    pub trend_window: usize,
    /// Categories to track
    pub categories: Vec<String>,
}

impl Default for BiasDetectionConfig {
    fn default() -> Self {
        Self {
            min_sample_size: 50,
            bias_threshold: 0.15,
            trend_window: 20,
            categories: vec![
                "finance".to_string(),
                "technology".to_string(),
                "health".to_string(),
                "politics".to_string(),
            ],
        }
    }
}

impl BiasDetector {
    pub fn new() -> Self {
        Self {
            biases: Vec::new(),
            config: BiasDetectionConfig::default(),
            category_accuracy: HashMap::new(),
        }
    }

    pub fn with_config(mut self, config: BiasDetectionConfig) -> Self {
        self.config = config;
        self
    }

    /// Update accuracy for a category
    pub fn record_category_accuracy(&mut self, category: &str, accuracy: f64) {
        self.category_accuracy
            .entry(category.to_string())
            .or_default()
            .push((Utc::now(), accuracy));
    }

    /// Detect calibration bias
    pub fn detect_calibration_bias(
        &self,
        trackers: &[PredictionTracker],
    ) -> Option<SystematicBias> {
        if trackers.len() < self.config.min_sample_size {
            return None;
        }

        let mut total_confidence = 0.0;
        let mut total_accuracy = 0.0;
        let mut count = 0;

        for tracker in trackers {
            if tracker.total_predictions < 10 {
                continue;
            }

            let avg_confidence: f64 = tracker.history.iter().map(|h| h.confidence).sum::<f64>()
                / tracker.history.len() as f64;
            let accuracy = tracker.current_accuracy();

            total_confidence += avg_confidence;
            total_accuracy += accuracy;
            count += 1;
        }

        if count == 0 {
            return None;
        }

        let avg_confidence = total_confidence / count as f64;
        let avg_accuracy = total_accuracy / count as f64;
        let calibration_error = (avg_confidence - avg_accuracy).abs();

        if calibration_error > self.config.bias_threshold {
            Some(SystematicBias {
                bias_type: BiasType::CalibrationError,
                direction: if avg_confidence > avg_accuracy {
                    BiasDirection::Positive
                } else {
                    BiasDirection::Negative
                },
                magnitude: calibration_error,
                affected_areas: vec!["all".to_string()],
                detected_at: Utc::now(),
                confidence: calibration_error.min(1.0),
                examples: vec![],
            })
        } else {
            None
        }
    }

    /// Detect outcome bias
    pub fn detect_outcome_bias(&self, trackers: &[PredictionTracker]) -> Option<SystematicBias> {
        // Check if certain outcomes are systematically preferred
        let mut outcome_counts: HashMap<String, usize> = HashMap::new();
        let mut total = 0;

        for tracker in trackers {
            for record in &tracker.history {
                *outcome_counts
                    .entry(record.predicted_value.clone())
                    .or_insert(0) += 1;
                total += 1;
            }
        }

        if total < self.config.min_sample_size {
            return None;
        }

        // Check for outcome imbalance
        let expected_frequency = 1.0 / outcome_counts.len() as f64;
        let max_frequency: f64 = outcome_counts
            .values()
            .map(|&c| c as f64 / total as f64)
            .fold(0.0, f64::max);

        let imbalance = max_frequency - expected_frequency;

        if imbalance > self.config.bias_threshold {
            let preferred_outcome = outcome_counts
                .iter()
                .max_by_key(|(_, c)| *c)
                .map(|(o, _)| o.clone())
                .unwrap_or_default();

            Some(SystematicBias {
                bias_type: BiasType::OutcomeBias,
                direction: BiasDirection::Neutral,
                magnitude: imbalance,
                affected_areas: vec!["predictions".to_string()],
                detected_at: Utc::now(),
                confidence: imbalance.min(1.0),
                examples: vec![BiasExample {
                    timestamp: Utc::now(),
                    context: "outcome distribution".to_string(),
                    prediction: format!("Preferring: {}", preferred_outcome),
                    actual: format!("Expected frequency: {:.2}", expected_frequency),
                    bias_manifestation: format!("Actual frequency: {:.2}", max_frequency),
                }],
            })
        } else {
            None
        }
    }

    /// Detect context bias (accuracy varies by category)
    pub fn detect_context_bias(&self) -> Option<SystematicBias> {
        let mut category_avg: HashMap<String, f64> = HashMap::new();

        for (category, history) in &self.category_accuracy {
            if history.len() < 10 {
                continue;
            }
            let recent: Vec<_> = history.iter().rev().take(10).collect();
            let avg: f64 = recent.iter().map(|(_, acc)| acc).sum::<f64>() / recent.len() as f64;
            category_avg.insert(category.clone(), avg);
        }

        if category_avg.len() < 2 {
            return None;
        }

        let accuracies: Vec<f64> = category_avg.values().copied().collect();
        let max_acc: f64 = accuracies.iter().fold(0.0_f64, |a, b| a.max(*b));
        let min_acc: f64 = accuracies.iter().fold(1.0_f64, |a, b| a.min(*b));
        let variance = max_acc - min_acc;

        if variance > self.config.bias_threshold {
            let worst_category = category_avg
                .iter()
                .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
                .map(|(c, _)| c.clone())
                .unwrap_or_default();

            Some(SystematicBias {
                bias_type: BiasType::ContextBias,
                direction: BiasDirection::Neutral,
                magnitude: variance,
                affected_areas: vec![worst_category],
                detected_at: Utc::now(),
                confidence: variance.min(1.0),
                examples: vec![],
            })
        } else {
            None
        }
    }

    /// Run all bias detection
    pub fn detect_all_biases(&mut self, trackers: &[PredictionTracker]) -> Vec<SystematicBias> {
        let mut new_biases = Vec::new();

        if let Some(bias) = self.detect_calibration_bias(trackers) {
            new_biases.push(bias);
        }

        if let Some(bias) = self.detect_outcome_bias(trackers) {
            new_biases.push(bias);
        }

        if let Some(bias) = self.detect_context_bias() {
            new_biases.push(bias);
        }

        self.biases.extend(new_biases.clone());
        new_biases
    }

    /// Get all detected biases
    pub fn biases(&self) -> &[SystematicBias] {
        &self.biases
    }

    /// Get active (recent) biases
    pub fn active_biases(&self, max_age: Duration) -> Vec<&SystematicBias> {
        let cutoff = Utc::now() - max_age;
        self.biases
            .iter()
            .filter(|b| b.detected_at > cutoff)
            .collect()
    }

    /// Clear old biases
    pub fn clear_old_biases(&mut self, max_age: Duration) {
        let cutoff = Utc::now() - max_age;
        self.biases.retain(|b| b.detected_at > cutoff);
    }
}

impl Default for BiasDetector {
    fn default() -> Self {
        Self::new()
    }
}
