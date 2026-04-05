//! Anomaly detector implementation

use super::*;
use chrono::Utc;

/// Anomaly detector for paradigm shift triggering
#[derive(Debug, Clone)]
pub struct AnomalyDetector {
    config: AnomalyConfig,
    history: Vec<SignalSnapshot>,
}

impl AnomalyDetector {
    pub fn new(config: AnomalyConfig) -> Self {
        Self {
            config,
            history: Vec::new(),
        }
    }

    /// Record a signal snapshot
    pub fn record_snapshot(&mut self, snapshot: SignalSnapshot) {
        self.history.push(snapshot);

        // Keep only recent history
        let cutoff = Utc::now() - chrono::Duration::hours(self.config.analysis_window_hours * 2);
        self.history.retain(|s| s.timestamp > cutoff);
    }

    /// Detect anomalies based on recent history
    pub fn detect(&self) -> AnomalyResult {
        if self.history.len() < self.config.min_samples {
            return AnomalyResult {
                is_anomaly: false,
                signals: vec![],
                severity: 0.0,
                recommendation: ShiftRecommendation::Continue,
            };
        }

        let recent: Vec<_> = self
            .history
            .iter()
            .filter(|s| {
                s.timestamp
                    > Utc::now() - chrono::Duration::hours(self.config.analysis_window_hours)
            })
            .collect();

        if recent.is_empty() {
            return AnomalyResult {
                is_anomaly: false,
                signals: vec![],
                severity: 0.0,
                recommendation: ShiftRecommendation::Continue,
            };
        }

        let mut signals = Vec::new();
        let mut severity: f64 = 0.0;

        // Check prediction error streak
        let avg_error =
            recent.iter().map(|s| s.prediction_error_rate).sum::<f64>() / recent.len() as f64;
        if avg_error > 0.5 {
            let streak = (avg_error * 10.0) as u32;
            if streak >= self.config.prediction_error_streak_threshold {
                signals.push(AnomalySignal::HighPredictionErrorStreak {
                    streak,
                    threshold: self.config.prediction_error_streak_threshold,
                });
                severity += 0.3;
            }
        }

        // Check conflict density
        let avg_conflict_density =
            recent.iter().map(|s| s.conflict_density).sum::<f64>() / recent.len() as f64;
        if avg_conflict_density > self.config.conflict_density_threshold {
            signals.push(AnomalySignal::ElevatedConflictDensity {
                density: avg_conflict_density,
                threshold: self.config.conflict_density_threshold,
            });
            severity += 0.25;
        }

        // Check cache hit rate trend
        if recent.len() >= 2 {
            let first_half = &recent[..recent.len() / 2];
            let second_half = &recent[recent.len() / 2..];

            let first_avg =
                first_half.iter().map(|s| s.cache_hit_rate).sum::<f64>() / first_half.len() as f64;
            let second_avg = second_half.iter().map(|s| s.cache_hit_rate).sum::<f64>()
                / second_half.len() as f64;

            let trend = (second_avg - first_avg) / first_avg;
            if trend < self.config.cache_hit_rate_decline_threshold {
                signals.push(AnomalySignal::DecliningCacheHitRate {
                    trend,
                    threshold: self.config.cache_hit_rate_decline_threshold,
                });
                severity += 0.25;
            }
        }

        // Check insight accuracy
        let avg_insight_accuracy = recent
            .iter()
            .filter_map(|s| {
                if s.insight_prediction_accuracy > 0.0 {
                    Some(s.insight_prediction_accuracy)
                } else {
                    None
                }
            })
            .sum::<f64>()
            / recent.len().max(1) as f64;

        if avg_insight_accuracy > 0.0 && avg_insight_accuracy < 0.6 {
            signals.push(AnomalySignal::LowInsightAccuracy {
                accuracy: avg_insight_accuracy,
                expected: 0.7,
            });
            severity += 0.2;
        }

        let recommendation = if severity >= 0.8 {
            ShiftRecommendation::ExecuteShift
        } else if severity >= 0.5 {
            ShiftRecommendation::PrepareShift
        } else if severity >= 0.2 {
            ShiftRecommendation::Monitor
        } else {
            ShiftRecommendation::Continue
        };

        AnomalyResult {
            is_anomaly: severity > 0.0,
            signals,
            severity: severity.min(1.0),
            recommendation,
        }
    }

    /// Get trend analysis for a specific metric
    pub fn get_trend(&self, metric: &str) -> Option<f64> {
        if self.history.len() < 2 {
            return None;
        }

        let values: Vec<f64> = match metric {
            "prediction_error_rate" => self
                .history
                .iter()
                .map(|s| s.prediction_error_rate)
                .collect(),
            "conflict_density" => self.history.iter().map(|s| s.conflict_density).collect(),
            "cache_hit_rate" => self.history.iter().map(|s| s.cache_hit_rate).collect(),
            "insight_prediction_accuracy" => self
                .history
                .iter()
                .map(|s| s.insight_prediction_accuracy)
                .collect(),
            _ => return None,
        };

        // Simple linear regression slope
        let n = values.len() as f64;
        let x_mean = (n - 1.0) / 2.0;
        let y_mean = values.iter().sum::<f64>() / n;

        let numerator: f64 = values
            .iter()
            .enumerate()
            .map(|(i, y)| (i as f64 - x_mean) * (y - y_mean))
            .sum();
        let denominator: f64 = values
            .iter()
            .enumerate()
            .map(|(i, _)| (i as f64 - x_mean).powi(2))
            .sum();

        if denominator == 0.0 {
            return Some(0.0);
        }

        Some(numerator / denominator)
    }
}
