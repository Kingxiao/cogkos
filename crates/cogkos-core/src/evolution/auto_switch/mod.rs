//! Automated Switch Controller - automates framework switching based on A/B test results
//!
//! This module provides:
//! 1. Automated switching logic triggered by A/B test results
//! 2. Gradual rollout management
//! 3. Rollback automation on failure detection
//! 4. Metrics collection and monitoring

pub mod controller;
pub mod integration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::models::{EvolutionEngineState, ShiftRecord, ShiftResult};

pub use controller::*;
pub use integration::*;

/// Errors for automated switching
#[derive(Error, Debug)]
pub enum AutoSwitchError {
    #[error("A/B test not found: {0}")]
    TestNotFound(String),
    #[error("Insufficient data: {0}")]
    InsufficientData(String),
    #[error("Switch rejected: {0}")]
    SwitchRejected(String),
    #[error("Rollback failed: {0}")]
    RollbackFailed(String),
}

pub type Result<T> = std::result::Result<T, AutoSwitchError>;

/// Configuration for automated switching
#[derive(Debug, Clone)]
pub struct AutoSwitchConfig {
    /// Minimum sample size before making decision
    pub min_sample_size: usize,
    /// Minimum improvement percentage to trigger switch
    pub min_improvement_pct: f64,
    /// Statistical significance threshold (p-value)
    pub significance_threshold: f64,
    /// Whether to enable automatic switching
    pub enabled: bool,
    /// Maximum rollback attempts
    pub max_rollback_attempts: u32,
    /// Rollback threshold - if performance drops below this, trigger rollback
    pub rollback_threshold_pct: f64,
    /// Gradual rollout: percentage of traffic to switch initially
    pub gradual_rollout_pct: f64,
    /// Time between gradual rollout increments
    pub rollout_interval_hours: i64,
}

impl Default for AutoSwitchConfig {
    fn default() -> Self {
        Self {
            min_sample_size: 1000,
            min_improvement_pct: 10.0,
            significance_threshold: 0.05,
            enabled: true,
            max_rollback_attempts: 3,
            rollback_threshold_pct: -5.0,
            gradual_rollout_pct: 0.1,
            rollout_interval_hours: 24,
        }
    }
}

/// Status of automated switching
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum AutoSwitchStatus {
    Disabled,
    Monitoring,
    Testing,
    GradualRollout,
    FullyDeployed,
    RollingBack,
}

/// Rollout phase
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum RolloutPhase {
    /// Not in rollout
    None,
    /// Initial rollout (10%)
    Initial,
    /// Partial rollout (25%)
    Partial,
    /// Majority rollout (50%)
    Majority,
    /// Full deployment (100%)
    Full,
}

impl RolloutPhase {
    pub fn percentage(&self) -> f64 {
        match self {
            RolloutPhase::None => 0.0,
            RolloutPhase::Initial => 0.1,
            RolloutPhase::Partial => 0.25,
            RolloutPhase::Majority => 0.5,
            RolloutPhase::Full => 1.0,
        }
    }

    pub fn next(&self) -> Self {
        match self {
            RolloutPhase::None => RolloutPhase::Initial,
            RolloutPhase::Initial => RolloutPhase::Partial,
            RolloutPhase::Partial => RolloutPhase::Majority,
            RolloutPhase::Majority => RolloutPhase::Full,
            RolloutPhase::Full => RolloutPhase::Full,
        }
    }
}

/// Test metrics for decision making
#[derive(Debug, Clone, Default)]
pub struct TestMetrics {
    pub control_correct: u64,
    pub control_total: u64,
    pub treatment_correct: u64,
    pub treatment_total: u64,
    pub control_avg_latency_ms: f64,
    pub treatment_avg_latency_ms: f64,
    pub control_conflicts: u64,
    pub treatment_conflicts: u64,
}

impl TestMetrics {
    pub fn control_accuracy(&self) -> f64 {
        if self.control_total == 0 {
            return 0.0;
        }
        self.control_correct as f64 / self.control_total as f64
    }

    pub fn treatment_accuracy(&self) -> f64 {
        if self.treatment_total == 0 {
            return 0.0;
        }
        self.treatment_correct as f64 / self.treatment_total as f64
    }

    pub fn improvement_pct(&self) -> f64 {
        let control = self.control_accuracy();
        if control == 0.0 {
            return 0.0;
        }
        (self.treatment_accuracy() - control) / control * 100.0
    }

    /// Calculate p-value using simplified z-test
    pub fn p_value(&self) -> f64 {
        let n1 = self.control_total as f64;
        let n2 = self.treatment_total as f64;
        if n1 == 0.0 || n2 == 0.0 {
            return 1.0;
        }

        let p1 = self.control_accuracy();
        let p2 = self.treatment_accuracy();

        // Pooled proportion
        let p = (self.control_correct + self.treatment_correct) as f64 / (n1 + n2);

        // Standard error
        let se = (p * (1.0 - p) * (1.0 / n1 + 1.0 / n2)).sqrt();

        if se == 0.0 {
            return 1.0;
        }

        // Z-score
        let z = (p2 - p1).abs() / se;

        // Simplified p-value approximation
        // For large samples, use normal approximation

        if z > 6.0 {
            1e-9
        } else if z > 3.0 {
            0.001
        } else if z > 2.0 {
            0.05
        } else if z > 1.5 {
            0.1
        } else {
            0.5
        }
    }

    pub fn is_significant(&self, threshold: f64) -> bool {
        self.p_value() < threshold
    }
}

/// Rollout reason
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RolloutReason {
    Initial,
    MetricsGood,
    ManualApproval,
    Rollback,
    Failed,
}

/// Record of a rollout change
#[derive(Debug, Clone)]
pub struct RolloutRecord {
    pub timestamp: DateTime<Utc>,
    pub from_phase: RolloutPhase,
    pub to_phase: RolloutPhase,
    pub reason: RolloutReason,
    pub test_metrics: TestMetrics,
}

/// Switch recommendation
#[derive(Debug, Clone)]
pub struct SwitchRecommendation {
    pub test_id: String,
    pub recommended_phase: RolloutPhase,
    pub improvement_pct: f64,
    pub p_value: f64,
    pub reason: String,
}

/// Actions to take based on evolution state
#[derive(Debug, Clone)]
pub enum EvolutionAction {
    /// Continue normal monitoring
    ContinueMonitoring,
    /// Initiate paradigm shift mode
    InitiateParadigmShift,
    /// Recommend framework switch
    RecommendSwitch(SwitchRecommendation),
    /// Execute rollback
    Rollback(RolloutReason),
}
