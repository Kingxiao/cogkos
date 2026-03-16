//! Framework Health Monitor
//!
//! Tracks the predictive power of Insights and reports systematic biases.
//! Part of Phase 5: Self-Reflection capabilities.

pub mod monitor;

use crate::models::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub use monitor::FrameworkHealthMonitor;

/// Health monitor configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthMonitorConfig {
    /// Window size for rolling statistics (hours)
    pub window_size_hours: i64,
    /// Minimum samples for reliable statistics
    pub min_samples: usize,
    /// Accuracy threshold for healthy predictions
    pub accuracy_threshold: f64,
    /// Bias detection threshold
    pub bias_threshold: f64,
    /// Enable systematic bias detection
    pub enable_bias_detection: bool,
}

impl Default for HealthMonitorConfig {
    fn default() -> Self {
        Self {
            window_size_hours: 168, // 1 week
            min_samples: 30,
            accuracy_threshold: 0.7,
            bias_threshold: 0.15,
            enable_bias_detection: true,
        }
    }
}

/// Tracking data for a single Insight
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightTrackingData {
    pub insight_id: Id,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub predictions: Vec<PredictionOutcome>,
    pub total_predictions: u32,
    pub correct_predictions: u32,
    pub accuracy_history: Vec<(DateTime<Utc>, f64)>,
}

/// Outcome of a prediction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionOutcome {
    pub predicted_at: DateTime<Utc>,
    pub predicted_value: String,
    pub actual_value: Option<String>,
    pub validated_at: Option<DateTime<Utc>>,
    pub error_score: Option<f64>,
    pub was_correct: Option<bool>,
}

/// Domain-specific metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DomainMetrics {
    pub domain: String,
    pub total_insights: u32,
    pub total_predictions: u32,
    pub correct_predictions: u32,
    pub avg_confidence: f64,
    pub avg_error: f64,
    pub prediction_count_by_type: HashMap<String, u32>,
}

/// Health snapshot at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthSnapshot {
    pub timestamp: DateTime<Utc>,
    pub overall_accuracy: f64,
    pub insight_count: usize,
    pub active_predictions: usize,
    pub avg_prediction_error: f64,
    pub domain_breakdown: HashMap<String, DomainHealth>,
    pub health_score: f64,
}

/// Domain health summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainHealth {
    pub domain: String,
    pub accuracy: f64,
    pub sample_size: u32,
    pub health_status: HealthStatus,
}

/// Health status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

/// Systematic bias report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystematicBias {
    pub bias_id: Id,
    pub detected_at: DateTime<Utc>,
    pub bias_type: BiasType,
    pub affected_domain: Option<String>,
    pub description: String,
    pub evidence: Vec<BiasEvidence>,
    pub magnitude: f64,
    pub confidence: f64,
    pub status: BiasStatus,
}

/// Types of systematic bias
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BiasType {
    Overconfidence,
    Underconfidence,
    TemporalBias,     // Favors recent data too much/too little
    SourceBias,       // Favors certain sources
    DomainBlindness,  // Poor performance in specific domain
    RecencyBias,      // Overweights recent predictions
    ConfirmationBias, // Only validates confirming evidence
}

/// Evidence for bias detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiasEvidence {
    pub metric: String,
    pub expected_value: f64,
    pub actual_value: f64,
    pub deviation: f64,
}

/// Bias status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BiasStatus {
    Open,
    Investigating,
    Mitigated,
    Ignored,
}

/// Health report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub generated_at: DateTime<Utc>,
    pub summary: HealthSummary,
    pub insights: Vec<InsightReport>,
    pub domain_breakdown: Vec<DomainReport>,
    pub biases: Vec<SystematicBias>,
    pub recommendations: Vec<HealthRecommendation>,
}

/// Health summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthSummary {
    pub overall_health_score: f64,
    pub status: HealthStatus,
    pub total_insights_tracked: usize,
    pub total_predictions: u32,
    pub overall_accuracy: f64,
    pub trend_direction: TrendDirection,
}

/// Trend direction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrendDirection {
    Improving,
    Stable,
    Declining,
    Unknown,
}

/// Insight performance report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightReport {
    pub insight_id: Id,
    pub content_preview: String,
    pub accuracy: f64,
    pub total_predictions: u32,
    pub trend: TrendDirection,
    pub status: HealthStatus,
}

/// Domain report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainReport {
    pub domain: String,
    pub accuracy: f64,
    pub sample_size: u32,
    pub status: HealthStatus,
    pub top_performing_insights: Vec<Id>,
    pub underperforming_insights: Vec<Id>,
}

/// Health recommendation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthRecommendation {
    pub priority: RecommendationPriority,
    pub category: RecommendationCategory,
    pub description: String,
    pub affected_insights: Vec<Id>,
    pub suggested_action: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecommendationPriority {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecommendationCategory {
    RetrainModel,
    ReviewData,
    AdjustThresholds,
    InvestigateBias,
    AddData,
    ArchiveInsight,
}
