//! Framework health monitor

use super::bias::*;
use super::tracker::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Framework health report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameworkHealthReport {
    pub generated_at: DateTime<Utc>,
    pub report_period_days: i64,
    /// Overall framework health score (0.0 - 1.0)
    pub overall_health: f64,
    /// Prediction power metrics
    pub prediction_power: PredictionPowerMetrics,
    /// Detected biases
    pub detected_biases: Vec<BiasReport>,
    /// Recommendations
    pub recommendations: Vec<String>,
    /// Status
    pub status: FrameworkStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionPowerMetrics {
    pub overall_accuracy: f64,
    pub accuracy_trend: TrendDirection,
    pub total_predictions_evaluated: u64,
    pub insights_evaluated: usize,
    pub high_performing_insights: usize,
    pub underperforming_insights: usize,
    pub calibration_score: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrendDirection {
    Improving,
    Stable,
    Declining,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiasReport {
    pub bias_type: String,
    pub direction: String,
    pub magnitude: f64,
    pub affected_areas: Vec<String>,
    pub detected_at: DateTime<Utc>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameworkStatus {
    Healthy,
    NeedsAttention,
    Degraded,
    Critical,
}

impl FrameworkHealthReport {
    pub fn is_healthy(&self) -> bool {
        self.status == FrameworkStatus::Healthy
    }
}

/// Framework health monitor
pub struct FrameworkHealthMonitor {
    /// Prediction trackers by insight
    trackers: HashMap<uuid::Uuid, PredictionTracker>,
    /// Bias detector
    bias_detector: BiasDetector,
    /// Report history
    report_history: Vec<FrameworkHealthReport>,
    /// Configuration
    config: MonitorConfig,
}

#[derive(Debug, Clone)]
pub struct MonitorConfig {
    /// How often to generate reports
    pub report_interval_hours: i64,
    /// Accuracy threshold for high performing
    pub high_performance_threshold: f64,
    /// Accuracy threshold for underperforming
    pub underperformance_threshold: f64,
    /// Max report history
    pub max_report_history: usize,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            report_interval_hours: 24,
            high_performance_threshold: 0.8,
            underperformance_threshold: 0.5,
            max_report_history: 100,
        }
    }
}

impl FrameworkHealthMonitor {
    pub fn new() -> Self {
        Self {
            trackers: HashMap::new(),
            bias_detector: BiasDetector::new(),
            report_history: Vec::new(),
            config: MonitorConfig::default(),
        }
    }

    pub fn with_config(mut self, config: MonitorConfig) -> Self {
        self.config = config;
        self
    }

    /// Record a prediction outcome
    pub fn record_prediction(
        &mut self,
        insight_id: uuid::Uuid,
        predicted: &str,
        actual: &str,
        confidence: f64,
    ) {
        self.trackers
            .entry(insight_id)
            .or_insert_with(|| PredictionTracker::new(insight_id))
            .record(predicted, actual, confidence);
    }

    /// Record category accuracy
    pub fn record_category_accuracy(&mut self, category: &str, accuracy: f64) {
        self.bias_detector
            .record_category_accuracy(category, accuracy);
    }

    /// Generate health report
    pub fn generate_report(&mut self, period_days: i64) -> FrameworkHealthReport {
        let trackers: Vec<_> = self.trackers.values().collect();

        // Calculate prediction power metrics
        let total_predictions: u64 = trackers.iter().map(|t| t.total_predictions).sum();
        let total_correct: u64 = trackers.iter().map(|t| t.correct_predictions).sum();
        let overall_accuracy = if total_predictions == 0 {
            0.0
        } else {
            total_correct as f64 / total_predictions as f64
        };

        // Determine trend
        let accuracy_trend = if trackers.iter().any(|t| t.is_declining(10)) {
            TrendDirection::Declining
        } else if trackers.len() >= 2 {
            let improving = trackers
                .iter()
                .filter(|t| t.rolling_accuracy(10) > t.current_accuracy())
                .count();
            let declining = trackers.iter().filter(|t| t.is_declining(10)).count();

            if improving > declining {
                TrendDirection::Improving
            } else if declining > improving {
                TrendDirection::Declining
            } else {
                TrendDirection::Stable
            }
        } else {
            TrendDirection::Unknown
        };

        // Count high/under performing
        let high_performing = trackers
            .iter()
            .filter(|t| t.current_accuracy() >= self.config.high_performance_threshold)
            .count();
        let underperforming = trackers
            .iter()
            .filter(|t| {
                t.current_accuracy() <= self.config.underperformance_threshold
                    && t.total_predictions >= 10
            })
            .count();

        // Calibration score (1.0 = perfect calibration)
        let calibration_score = 1.0
            - trackers
                .iter()
                .filter(|t| t.total_predictions > 0)
                .map(|t| {
                    let avg_confidence: f64 = t.history.iter().map(|h| h.confidence).sum::<f64>()
                        / t.history.len().max(1) as f64;
                    (avg_confidence - t.current_accuracy()).abs()
                })
                .sum::<f64>()
                / trackers.len().max(1) as f64;

        // Detect biases
        let trackers_vec: Vec<_> = trackers.into_iter().cloned().collect();
        let new_biases = self.bias_detector.detect_all_biases(&trackers_vec);

        let bias_reports: Vec<_> = new_biases
            .iter()
            .map(|b| BiasReport {
                bias_type: format!("{:?}", b.bias_type),
                direction: format!("{:?}", b.direction),
                magnitude: b.magnitude,
                affected_areas: b.affected_areas.clone(),
                detected_at: b.detected_at,
                is_active: true,
            })
            .collect();

        // Generate recommendations
        let mut recommendations = Vec::new();

        if accuracy_trend == TrendDirection::Declining {
            recommendations.push("Prediction accuracy is declining - review framework".to_string());
        }

        if calibration_score < 0.7 {
            recommendations
                .push("Poor confidence calibration - retrain confidence estimation".to_string());
        }

        for bias in &bias_reports {
            recommendations.push(format!(
                "Detected {} bias in {} - review model training data",
                bias.bias_type,
                bias.affected_areas.join(", ")
            ));
        }

        if underperforming > high_performing {
            recommendations
                .push("Too many underperforming insights - consider framework update".to_string());
        }

        // Determine status
        let status = if overall_accuracy < 0.5 || !bias_reports.is_empty() {
            FrameworkStatus::Critical
        } else if overall_accuracy < 0.7 || accuracy_trend == TrendDirection::Declining {
            FrameworkStatus::Degraded
        } else if !recommendations.is_empty() {
            FrameworkStatus::NeedsAttention
        } else {
            FrameworkStatus::Healthy
        };

        let report = FrameworkHealthReport {
            generated_at: Utc::now(),
            report_period_days: period_days,
            overall_health: overall_accuracy * calibration_score,
            prediction_power: PredictionPowerMetrics {
                overall_accuracy,
                accuracy_trend,
                total_predictions_evaluated: total_predictions,
                insights_evaluated: self.trackers.len(),
                high_performing_insights: high_performing,
                underperforming_insights: underperforming,
                calibration_score,
            },
            detected_biases: bias_reports,
            recommendations,
            status,
        };

        self.report_history.push(report.clone());
        if self.report_history.len() > self.config.max_report_history {
            self.report_history.remove(0);
        }

        report
    }

    /// Get report history
    pub fn report_history(&self) -> &[FrameworkHealthReport] {
        &self.report_history
    }

    /// Get tracker for an insight
    pub fn get_tracker(&self, insight_id: uuid::Uuid) -> Option<&PredictionTracker> {
        self.trackers.get(&insight_id)
    }

    /// Get all trackers
    pub fn trackers(&self) -> &HashMap<uuid::Uuid, PredictionTracker> {
        &self.trackers
    }
}

impl Default for FrameworkHealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}
