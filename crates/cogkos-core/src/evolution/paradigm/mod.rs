//! Paradigm Shift Module - L2 Evolution Engine (Paradigm Shift Mode)
//!
//! This module implements the paradigm shift functionality for CogKOS:
//! - Anomaly detection for triggering paradigm shifts
//! - LLM sandbox environment for safe experimentation
//! - A/B testing framework for comparing frameworks
//! - Switch/rollback mechanisms

pub mod anomaly;
pub mod orchestrator;
pub mod sandbox;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use anomaly::*;
pub use orchestrator::*;
pub use sandbox::*;

/// Configuration for anomaly detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyConfig {
    /// Prediction error streak threshold
    pub prediction_error_streak_threshold: u32,
    /// Conflict density threshold (0.0 - 1.0)
    pub conflict_density_threshold: f64,
    /// Cache hit rate decline threshold (negative percentage)
    pub cache_hit_rate_decline_threshold: f64,
    /// Minimum samples before triggering
    pub min_samples: usize,
    /// Time window for analysis (hours)
    pub analysis_window_hours: i64,
    /// Consecutive anomaly periods required
    pub consecutive_periods_required: u32,
}

impl Default for AnomalyConfig {
    fn default() -> Self {
        Self {
            prediction_error_streak_threshold: 5,
            conflict_density_threshold: 0.3,
            cache_hit_rate_decline_threshold: -0.2,
            min_samples: 100,
            analysis_window_hours: 24,
            consecutive_periods_required: 3,
        }
    }
}

/// Signal snapshot for tracking system health
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalSnapshot {
    pub timestamp: DateTime<Utc>,
    pub prediction_error_rate: f64,
    pub conflict_density: f64,
    pub cache_hit_rate: f64,
    pub avg_prediction_error: f64,
    pub insight_prediction_accuracy: f64,
    pub total_claims: usize,
    pub active_conflicts: usize,
}

/// Anomaly detection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyResult {
    pub is_anomaly: bool,
    pub signals: Vec<AnomalySignal>,
    pub severity: f64,
    pub recommendation: ShiftRecommendation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnomalySignal {
    HighPredictionErrorStreak { streak: u32, threshold: u32 },
    ElevatedConflictDensity { density: f64, threshold: f64 },
    DecliningCacheHitRate { trend: f64, threshold: f64 },
    LowInsightAccuracy { accuracy: f64, expected: f64 },
    SystematicBiasDetected { bias_type: String, magnitude: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShiftRecommendation {
    Continue,     // No action needed
    Monitor,      // Watch closely
    PrepareShift, // Start preparing paradigm shift
    ExecuteShift, // Execute shift immediately
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub max_experiments: usize,
    pub max_duration_minutes: u64,
    pub isolation_level: IsolationLevel,
    pub resource_limits: ResourceLimits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IsolationLevel {
    Process,
    Container,
    Vm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub max_tokens: u64,
    pub max_requests: u32,
    pub memory_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxExperiment {
    pub id: String,
    pub name: String,
    pub framework_variant: FrameworkVariant,
    pub status: ExperimentStatus,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub results: Option<ExperimentResults>,
    pub resource_usage: ResourceUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameworkVariant {
    pub id: String,
    pub name: String,
    pub prompt_template: String,
    pub aggregation_strategy: AggregationStrategy,
    pub conflict_resolution: ConflictResolutionStrategy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggregationStrategy {
    Bayesian,
    WeightedAverage,
    TrustPropagation,
    Neural,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictResolutionStrategy {
    TemporalPriority,
    SourceAuthority,
    ConfidenceWeighted,
    HumanReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExperimentStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentResults {
    pub prediction_accuracy: f64,
    pub conflict_resolution_rate: f64,
    pub avg_confidence: f64,
    pub processing_time_ms: u64,
    pub sample_size: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceUsage {
    pub tokens_used: u64,
    pub requests_made: u32,
    pub memory_mb: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShiftState {
    Normal,
    Detecting,
    Experimenting,
    Evaluating,
    Switching,
    RollingBack,
}

/// Actions the orchestrator can take
#[derive(Debug, Clone)]
pub enum ShiftAction {
    None,
    Wait,
    StartInvestigation(AnomalyResult),
    StartExperiment(String),
    EvaluateExperiment(String),
    ExecuteSwitch(String),
    AbortExperiment,
    SwitchComplete,
    RollbackComplete,
    Error(String),
}
