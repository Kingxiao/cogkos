use serde::{Deserialize, Serialize};

/// Evolution engine state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionEngineState {
    pub mode: EvolutionMode,
    pub anomaly_counter: u32,
    pub paradigm_shift_threshold: u32,
    pub ticks_since_last_shift: u32,
    #[serde(default)]
    pub shift_history: Vec<ShiftRecord>,
}

impl Default for EvolutionEngineState {
    fn default() -> Self {
        Self {
            mode: EvolutionMode::Incremental,
            anomaly_counter: 0,
            paradigm_shift_threshold: 10,
            ticks_since_last_shift: 0,
            shift_history: Vec::new(),
        }
    }
}

/// Evolution mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum EvolutionMode {
    Incremental,
    ParadigmShift,
}

/// Record of a paradigm shift
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftRecord {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub result: ShiftResult,
    pub old_framework_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_framework_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub improvement_pct: Option<f64>,
}

/// Result of a shift attempt
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ShiftResult {
    Success,
    Rollback,
}

/// Anomaly signals for triggering paradigm shift
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalySignals {
    pub prediction_error_streak: u32,
    pub conflict_density_pct: f64,
    pub cache_hit_rate_trend: f64,
}
