//! Framework Health Monitoring - Prediction accuracy tracking

use chrono::{DateTime, Utc};

/// Prediction accuracy tracking for insights
#[derive(Debug, Clone)]
pub struct PredictionTracker {
    /// Insight ID
    pub insight_id: uuid::Uuid,
    /// Total predictions made
    pub total_predictions: u64,
    /// Correct predictions
    pub correct_predictions: u64,
    /// Prediction history
    pub history: Vec<PredictionRecord>,
    /// Accuracy trend
    pub accuracy_trend: Vec<(DateTime<Utc>, f64)>,
}

#[derive(Debug, Clone)]
pub struct PredictionRecord {
    pub timestamp: DateTime<Utc>,
    pub predicted_value: String,
    pub actual_value: String,
    pub was_correct: bool,
    pub confidence: f64,
    pub error_margin: f64,
}

impl PredictionTracker {
    pub fn new(insight_id: uuid::Uuid) -> Self {
        Self {
            insight_id,
            total_predictions: 0,
            correct_predictions: 0,
            history: Vec::new(),
            accuracy_trend: Vec::new(),
        }
    }

    /// Record a prediction outcome
    pub fn record(&mut self, predicted: &str, actual: &str, confidence: f64) {
        let was_correct = predicted == actual;
        let error_margin = if was_correct { 0.0 } else { 1.0 };

        self.total_predictions += 1;
        if was_correct {
            self.correct_predictions += 1;
        }

        self.history.push(PredictionRecord {
            timestamp: Utc::now(),
            predicted_value: predicted.to_string(),
            actual_value: actual.to_string(),
            was_correct,
            confidence,
            error_margin,
        });

        // Update trend every 10 predictions
        if self.total_predictions.is_multiple_of(10) {
            let accuracy = self.current_accuracy();
            self.accuracy_trend.push((Utc::now(), accuracy));
        }
    }

    /// Get current accuracy
    pub fn current_accuracy(&self) -> f64 {
        if self.total_predictions == 0 {
            0.0
        } else {
            self.correct_predictions as f64 / self.total_predictions as f64
        }
    }

    /// Get rolling accuracy over last N predictions
    pub fn rolling_accuracy(&self, window: usize) -> f64 {
        let window = window.min(self.history.len());
        if window == 0 {
            return 0.0;
        }

        let recent: Vec<_> = self.history.iter().rev().take(window).collect();
        let correct = recent.iter().filter(|r| r.was_correct).count();

        correct as f64 / window as f64
    }

    /// Check if accuracy is declining
    pub fn is_declining(&self, window: usize) -> bool {
        if self.accuracy_trend.len() < 2 {
            return false;
        }

        let recent: Vec<_> = self.accuracy_trend.iter().rev().take(window).collect();
        if recent.len() < 2 {
            return false;
        }

        let first = recent[recent.len() - 1].1;
        let last = recent[0].1;

        last < first - 0.1 // Declined by more than 10%
    }
}
