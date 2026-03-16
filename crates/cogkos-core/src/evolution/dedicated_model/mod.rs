//! Dedicated Prediction Models
//!
//! Domain-specific model training logic, data sufficiency detection,
//! and model switching mechanisms.
//! Part of Phase 5: Self-Reflection capabilities.

pub mod comparison;
pub mod manager;

use crate::models::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub use comparison::*;
pub use manager::*;

/// Configuration for dedicated prediction models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionModelConfig {
    /// Minimum samples required for training
    pub min_samples_for_training: usize,
    /// Minimum accuracy to consider a model viable
    pub min_accuracy_threshold: f64,
    /// Retraining interval (hours)
    pub retraining_interval_hours: i64,
    /// Maximum number of domain models
    pub max_domain_models: usize,
    /// Whether to enable automatic model switching
    pub enable_auto_switch: bool,
    /// Improvement threshold for switching (percentage)
    pub switch_improvement_threshold: f64,
}

impl Default for PredictionModelConfig {
    fn default() -> Self {
        Self {
            min_samples_for_training: 100,
            min_accuracy_threshold: 0.7,
            retraining_interval_hours: 168, // 1 week
            max_domain_models: 10,
            enable_auto_switch: true,
            switch_improvement_threshold: 0.05, // 5% improvement
        }
    }
}

/// Domain-specific prediction model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainModel {
    pub model_id: String,
    pub domain: String,
    pub version: u32,
    pub status: ModelStatus,
    pub training_data: TrainingDataInfo,
    pub performance: ModelPerformance,
    pub created_at: DateTime<Utc>,
    pub last_trained_at: DateTime<Utc>,
    pub model_type: ModelType,
    pub hyperparameters: HashMap<String, f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelStatus {
    Training,
    Ready,
    Deployed,
    Deprecated,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelType {
    Statistical,
    Neural,
    Ensemble,
    RuleBased,
}

/// Training data information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingDataInfo {
    pub sample_count: usize,
    pub feature_count: usize,
    pub label_distribution: HashMap<String, u32>,
    pub time_range: (DateTime<Utc>, DateTime<Utc>),
    pub quality_score: f64,
}

/// Model performance metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelPerformance {
    pub accuracy: f64,
    pub precision: f64,
    pub recall: f64,
    pub f1_score: f64,
    pub training_time_ms: u64,
    pub inference_time_ms: u64,
    pub validation_loss: f64,
    pub confidence_calibration: f64,
}

/// Model metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub model_id: String,
    pub model_type: ModelType,
    pub description: String,
}

/// Training job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingJob {
    pub job_id: String,
    pub domain: String,
    pub status: TrainingStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub data_stats: DataStats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrainingStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Data statistics for training
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataStats {
    pub available_samples: usize,
    pub unique_features: usize,
    pub class_balance: f64,
}

/// Model performance record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPerformanceRecord {
    pub timestamp: DateTime<Utc>,
    pub model_id: String,
    pub domain: String,
    pub accuracy: f64,
    pub sample_count: u32,
}

/// Data sufficiency analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSufficiencyResult {
    pub is_sufficient: bool,
    pub sample_count: usize,
    pub required_samples: usize,
    pub quality_score: f64,
    pub recommendations: Vec<DataRecommendation>,
    pub estimated_accuracy: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRecommendation {
    pub recommendation_type: DataRecType,
    pub description: String,
    pub impact: ImpactLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataRecType {
    CollectMoreData,
    BalanceClasses,
    AddFeatures,
    CleanData,
    ReduceFeatures,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImpactLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// Model switch decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSwitchDecision {
    pub should_switch: bool,
    pub current_model: String,
    pub proposed_model: String,
    pub improvement: f64,
    pub confidence: f64,
    pub reason: String,
}

/// Prediction result from dedicated model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DedicatedPrediction {
    pub model_id: String,
    pub prediction: String,
    pub confidence: f64,
    pub alternative_predictions: Vec<(String, f64)>,
    pub inference_time_ms: u64,
    pub feature_importance: HashMap<String, f64>,
}

/// Feature vector for prediction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureVector {
    pub features: HashMap<String, FeatureValue>,
    pub context: PredictionContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FeatureValue {
    Numeric(f64),
    Categorical(String),
    Boolean(bool),
    Vector(Vec<f64>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionContext {
    pub domain: String,
    pub tenant_id: String,
    pub related_claims: Vec<Id>,
    pub temporal_context: Option<TemporalContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalContext {
    pub prediction_time: DateTime<Utc>,
    pub relevant_timeframe: (DateTime<Utc>, DateTime<Utc>),
}

/// Training sample
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingSample {
    pub id: String,
    pub features: HashMap<String, FeatureValue>,
    pub label: String,
    pub timestamp: DateTime<Utc>,
    pub weight: f64,
}

impl TrainingSample {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            features: HashMap::new(),
            label: label.into(),
            timestamp: Utc::now(),
            weight: 1.0,
        }
    }

    pub fn with_feature(mut self, name: impl Into<String>, value: FeatureValue) -> Self {
        self.features.insert(name.into(), value);
        self
    }
}
