//! Anomaly detection for paradigm shift triggering

use chrono::{DateTime, Utc};
use thiserror::Error;
use uuid::Uuid;

use crate::models::EpistemicClaim;

/// Paradigm shift errors
#[derive(Error, Debug)]
pub enum ParadigmShiftError {
    #[error("Anomaly detection failed: {0}")]
    AnomalyDetectionFailed(String),
    #[error("Sandbox error: {0}")]
    SandboxError(String),
    #[error("A/B test error: {0}")]
    ABTestError(String),
    #[error("Framework validation failed: {0}")]
    ValidationFailed(String),
    #[error("Rollback failed: {0}")]
    RollbackFailed(String),
    #[error("Insufficient data: {0}")]
    InsufficientData(String),
}

pub type Result<T> = std::result::Result<T, ParadigmShiftError>;

/// Anomaly detection result
#[derive(Debug, Clone)]
pub struct AnomalyDetectionResult {
    /// Anomaly score (0.0 - 1.0), higher = more anomalous
    pub anomaly_score: f64,
    /// Detected anomalies
    pub anomalies: Vec<Anomaly>,
    /// Overall assessment
    pub assessment: AnomalyAssessment,
    /// Recommended action
    pub recommendation: AnomalyRecommendation,
}

/// Individual anomaly
#[derive(Debug, Clone)]
pub struct Anomaly {
    /// Type of anomaly
    pub anomaly_type: AnomalyType,
    /// Affected claims
    pub claim_ids: Vec<Uuid>,
    /// Description
    pub description: String,
    /// Confidence in detection
    pub confidence: f64,
    /// Deviation from expected pattern
    pub deviation_score: f64,
}

/// Types of anomalies
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyType {
    /// Conflicts that don't fit current resolution rules
    ResolutionConflict,
    /// Predictions consistently failing
    PredictionFailure,
    /// Knowledge structure doesn't fit current ontology
    OntologyMismatch,
    /// Confidence calibration is off
    CalibrationDrift,
    /// Semantic drift in key terms
    SemanticDrift,
    /// Coverage gap in critical domain
    CoverageGap,
}

/// Overall anomaly assessment
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyAssessment {
    Normal,
    Elevated,
    Critical,
    ParadigmBreaking,
}

/// Recommended action
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyRecommendation {
    NoAction,
    Monitor,
    ReviewFramework,
    InitiateParadigmShift,
}

/// Anomaly detector
pub struct AnomalyDetector {
    /// Threshold for considering an anomaly significant
    _threshold: f64,
    /// History of anomaly scores
    history: Vec<(DateTime<Utc>, f64)>,
    /// Pattern detector configuration
    config: AnomalyConfig,
}

#[derive(Debug, Clone)]
pub struct AnomalyConfig {
    /// Prediction error threshold
    pub prediction_error_threshold: f64,
    /// Minimum sample size for detection
    pub min_sample_size: usize,
    /// Conflict density threshold
    pub conflict_density_threshold: f64,
    /// Semantic drift threshold
    pub semantic_drift_threshold: f64,
    /// Calibration drift threshold
    pub calibration_drift_threshold: f64,
}

impl Default for AnomalyConfig {
    fn default() -> Self {
        Self {
            prediction_error_threshold: 0.3,
            min_sample_size: 100,
            conflict_density_threshold: 0.2,
            semantic_drift_threshold: 0.25,
            calibration_drift_threshold: 0.15,
        }
    }
}

impl AnomalyDetector {
    pub fn new(threshold: f64) -> Self {
        Self {
            _threshold: threshold,
            history: Vec::new(),
            config: AnomalyConfig::default(),
        }
    }

    pub fn with_config(mut self, config: AnomalyConfig) -> Self {
        self.config = config;
        self
    }

    /// Detect anomalies in recent claims
    pub fn detect(
        &mut self,
        claims: &[EpistemicClaim],
        prediction_errors: &[f64],
    ) -> AnomalyDetectionResult {
        let mut anomalies = Vec::new();

        // Check prediction failures
        if prediction_errors.len() >= self.config.min_sample_size {
            let avg_error: f64 =
                prediction_errors.iter().sum::<f64>() / prediction_errors.len() as f64;
            if avg_error > self.config.prediction_error_threshold {
                anomalies.push(Anomaly {
                    anomaly_type: AnomalyType::PredictionFailure,
                    claim_ids: claims.iter().take(10).map(|c| c.id).collect(),
                    description: format!(
                        "Average prediction error {:.2} exceeds threshold",
                        avg_error
                    ),
                    confidence: (avg_error / self.config.prediction_error_threshold).min(1.0),
                    deviation_score: avg_error,
                });
            }
        }

        // Check conflict density
        let contested_count = claims
            .iter()
            .filter(|c| {
                matches!(
                    c.epistemic_status,
                    crate::models::EpistemicStatus::Contested
                )
            })
            .count();
        let conflict_density = if claims.is_empty() {
            0.0
        } else {
            contested_count as f64 / claims.len() as f64
        };

        if conflict_density > self.config.conflict_density_threshold {
            anomalies.push(Anomaly {
                anomaly_type: AnomalyType::ResolutionConflict,
                claim_ids: claims
                    .iter()
                    .filter(|c| {
                        matches!(
                            c.epistemic_status,
                            crate::models::EpistemicStatus::Contested
                        )
                    })
                    .take(10)
                    .map(|c| c.id)
                    .collect(),
                description: format!("Conflict density {:.2} exceeds threshold", conflict_density),
                confidence: (conflict_density / self.config.conflict_density_threshold).min(1.0),
                deviation_score: conflict_density,
            });
        }

        // Check confidence calibration
        let high_confidence_wrong: Vec<_> = claims
            .iter()
            .filter(|c| {
                c.confidence > 0.8 && c.last_prediction_error.map(|e| e > 0.5).unwrap_or(false)
            })
            .collect();

        if high_confidence_wrong.len() as f64 / claims.len().max(1) as f64
            > self.config.calibration_drift_threshold
        {
            anomalies.push(Anomaly {
                anomaly_type: AnomalyType::CalibrationDrift,
                claim_ids: high_confidence_wrong
                    .iter()
                    .take(10)
                    .map(|c| c.id)
                    .collect(),
                description: "High confidence predictions frequently wrong".to_string(),
                confidence: 0.8,
                deviation_score: high_confidence_wrong.len() as f64 / claims.len() as f64,
            });
        }

        // Calculate overall anomaly score
        let anomaly_score = if anomalies.is_empty() {
            0.0
        } else {
            let max_confidence: f64 = anomalies.iter().map(|a| a.confidence).fold(0.0, f64::max);
            let avg_deviation: f64 =
                anomalies.iter().map(|a| a.deviation_score).sum::<f64>() / anomalies.len() as f64;
            (max_confidence * 0.6 + avg_deviation.min(1.0) * 0.4).min(1.0)
        };

        // Record in history
        self.history.push((Utc::now(), anomaly_score));
        if self.history.len() > 1000 {
            self.history.remove(0);
        }

        // Determine assessment
        let assessment = if anomaly_score >= 0.8 {
            AnomalyAssessment::ParadigmBreaking
        } else if anomaly_score > 0.6 {
            AnomalyAssessment::Critical
        } else if anomaly_score > 0.3 {
            AnomalyAssessment::Elevated
        } else {
            AnomalyAssessment::Normal
        };

        // Determine recommendation
        let recommendation = match assessment {
            AnomalyAssessment::ParadigmBreaking => AnomalyRecommendation::InitiateParadigmShift,
            AnomalyAssessment::Critical => AnomalyRecommendation::ReviewFramework,
            AnomalyAssessment::Elevated => AnomalyRecommendation::Monitor,
            AnomalyAssessment::Normal => AnomalyRecommendation::NoAction,
        };

        AnomalyDetectionResult {
            anomaly_score,
            anomalies,
            assessment,
            recommendation,
        }
    }

    /// Check if there's a persistent anomaly trend
    pub fn has_persistent_anomaly(&self, window_size: usize, threshold: f64) -> bool {
        if self.history.len() < window_size {
            return false;
        }

        let recent: Vec<f64> = self
            .history
            .iter()
            .rev()
            .take(window_size)
            .map(|(_, s)| *s)
            .collect();
        let avg = recent.iter().sum::<f64>() / recent.len() as f64;

        avg > threshold
    }

    /// Get anomaly trend (positive = worsening)
    pub fn trend(&self, window_size: usize) -> f64 {
        if self.history.len() < window_size * 2 {
            return 0.0;
        }

        let recent: Vec<f64> = self
            .history
            .iter()
            .rev()
            .take(window_size)
            .map(|(_, s)| *s)
            .collect();
        let older: Vec<f64> = self
            .history
            .iter()
            .rev()
            .skip(window_size)
            .take(window_size)
            .map(|(_, s)| *s)
            .collect();

        let recent_avg = recent.iter().sum::<f64>() / recent.len() as f64;
        let older_avg = older.iter().sum::<f64>() / older.len() as f64;

        recent_avg - older_avg
    }
}
