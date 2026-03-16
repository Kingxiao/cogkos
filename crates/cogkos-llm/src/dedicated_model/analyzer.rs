//! Dedicated Prediction Models - Data sufficiency analysis

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Dedicated model errors
#[derive(Error, Debug)]
pub enum DedicatedModelError {
    #[error("Insufficient data: {0}")]
    InsufficientData(String),
    #[error("Training failed: {0}")]
    TrainingFailed(String),
    #[error("Model not found: {0}")]
    ModelNotFound(String),
    #[error("Switch failed: {0}")]
    SwitchFailed(String),
    #[error("Validation failed: {0}")]
    ValidationFailed(String),
}

pub type Result<T> = std::result::Result<T, DedicatedModelError>;

/// Model architecture types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelArchitecture {
    /// Small fine-tuned model
    FineTunedSmall,
    /// Medium fine-tuned model
    FineTunedMedium,
    /// Large fine-tuned model
    FineTunedLarge,
    /// LoRA adapter
    LoraAdapter,
    /// Prompt-tuned model
    PromptTuned,
    /// RAG-enhanced
    RagEnhanced,
}

/// Training data requirements
#[derive(Debug, Clone)]
pub struct DataRequirements {
    /// Minimum samples needed
    pub min_samples: usize,
    /// Ideal samples for good performance
    pub ideal_samples: usize,
    /// Minimum samples per class/category
    pub min_samples_per_class: usize,
    /// Required diversity score
    pub min_diversity_score: f64,
}

impl Default for DataRequirements {
    fn default() -> Self {
        Self {
            min_samples: 100,
            ideal_samples: 1000,
            min_samples_per_class: 10,
            min_diversity_score: 0.6,
        }
    }
}

/// Data sufficiency analysis result
#[derive(Debug, Clone)]
pub struct DataSufficiencyResult {
    pub is_sufficient: bool,
    pub sample_count: usize,
    pub samples_per_class: HashMap<String, usize>,
    pub diversity_score: f64,
    pub gaps: Vec<DataGap>,
    pub recommendation: DataRecommendation,
    pub estimated_performance: f64,
}

#[derive(Debug, Clone)]
pub struct DataGap {
    pub gap_type: GapType,
    pub description: String,
    pub severity: GapSeverity,
    pub suggested_action: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapType {
    InsufficientSamples,
    ClassImbalance,
    LowDiversity,
    MissingFeatures,
    QualityIssues,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone)]
pub enum DataRecommendation {
    ProceedWithTraining,
    CollectMoreData { target_samples: usize },
    BalanceClasses { target_per_class: usize },
    ImproveDiversity,
    NotRecommended { reason: String },
}

/// Training sample
#[derive(Debug, Clone)]
pub struct TrainingSample {
    pub id: String,
    pub features: Vec<f64>,
    pub class_label: String,
    pub metadata: HashMap<String, String>,
    pub timestamp: DateTime<Utc>,
}

/// Dedicated prediction model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DedicatedPredictionModel {
    pub model_id: String,
    pub domain: String,
    pub architecture: ModelArchitecture,
    pub status: ModelStatus,
    pub version: u32,
    pub training_info: TrainingInfo,
    pub performance: ModelPerformance,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub hyperparameters: HashMap<String, f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelStatus {
    Pending,
    Training,
    Validating,
    Ready,
    Deployed,
    Deprecated,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingInfo {
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub sample_count: usize,
    pub training_duration_seconds: u64,
    pub data_hash: String,
    pub validation_split: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPerformance {
    pub accuracy: f64,
    pub precision: f64,
    pub recall: f64,
    pub f1_score: f64,
    pub confidence_calibration: f64,
    pub latency_ms: u64,
    pub evaluated_at: DateTime<Utc>,
}

impl Default for ModelPerformance {
    fn default() -> Self {
        Self {
            accuracy: 0.0,
            precision: 0.0,
            recall: 0.0,
            f1_score: 0.0,
            confidence_calibration: 0.0,
            latency_ms: 0,
            evaluated_at: Utc::now(),
        }
    }
}

/// Data sufficiency analyzer
pub struct DataSufficiencyAnalyzer {
    requirements: DataRequirements,
}

impl DataSufficiencyAnalyzer {
    pub fn new() -> Self {
        Self {
            requirements: DataRequirements::default(),
        }
    }

    pub fn with_requirements(mut self, requirements: DataRequirements) -> Self {
        self.requirements = requirements;
        self
    }

    /// Analyze if data is sufficient for training
    pub fn analyze(&self, samples: &[TrainingSample]) -> DataSufficiencyResult {
        let sample_count = samples.len();

        // Count samples per class
        let mut class_counts: HashMap<String, usize> = HashMap::new();
        for sample in samples {
            *class_counts.entry(sample.class_label.clone()).or_insert(0) += 1;
        }

        // Calculate diversity score
        let diversity_score = self.calculate_diversity(samples);

        // Identify gaps
        let mut gaps = Vec::new();
        let mut is_sufficient = true;

        if sample_count < self.requirements.min_samples {
            gaps.push(DataGap {
                gap_type: GapType::InsufficientSamples,
                description: format!(
                    "Only {} samples, need {}",
                    sample_count, self.requirements.min_samples
                ),
                severity: if sample_count < self.requirements.min_samples / 2 {
                    GapSeverity::Critical
                } else {
                    GapSeverity::High
                },
                suggested_action: format!(
                    "Collect at least {} more samples",
                    self.requirements.min_samples - sample_count
                ),
            });
            is_sufficient = false;
        }

        for (class, count) in &class_counts {
            if *count < self.requirements.min_samples_per_class {
                gaps.push(DataGap {
                    gap_type: GapType::ClassImbalance,
                    description: format!("Class '{}' has only {} samples", class, count),
                    severity: GapSeverity::Medium,
                    suggested_action: format!("Collect more samples for class '{}'", class),
                });
            }
        }

        if diversity_score < self.requirements.min_diversity_score {
            gaps.push(DataGap {
                gap_type: GapType::LowDiversity,
                description: format!(
                    "Diversity score {:.2} below threshold {:.2}",
                    diversity_score, self.requirements.min_diversity_score
                ),
                severity: GapSeverity::Medium,
                suggested_action: "Add more diverse training examples".to_string(),
            });
            is_sufficient = false;
        }

        // Determine recommendation
        let recommendation = if is_sufficient {
            DataRecommendation::ProceedWithTraining
        } else if sample_count < self.requirements.min_samples {
            DataRecommendation::CollectMoreData {
                target_samples: self.requirements.min_samples,
            }
        } else if diversity_score < self.requirements.min_diversity_score {
            DataRecommendation::ImproveDiversity
        } else {
            DataRecommendation::BalanceClasses {
                target_per_class: self.requirements.min_samples_per_class,
            }
        };

        // Estimate performance based on data quality
        let estimated_performance =
            self.estimate_performance(sample_count, diversity_score, &class_counts);

        DataSufficiencyResult {
            is_sufficient,
            sample_count,
            samples_per_class: class_counts,
            diversity_score,
            gaps,
            recommendation,
            estimated_performance,
        }
    }

    /// Calculate diversity score
    fn calculate_diversity(&self, samples: &[TrainingSample]) -> f64 {
        if samples.len() < 2 {
            return 0.0;
        }

        // Simple diversity: unique feature combinations / total samples
        // Use string representation since f64 doesn't implement Hash/Eq
        let unique_features: std::collections::HashSet<String> = samples
            .iter()
            .map(|s| format!("{:?}", s.features))
            .collect();

        unique_features.len() as f64 / samples.len() as f64
    }

    /// Estimate model performance based on data
    pub(crate) fn estimate_performance(
        &self,
        sample_count: usize,
        diversity: f64,
        class_counts: &HashMap<String, usize>,
    ) -> f64 {
        // Heuristic: more data + higher diversity = better performance
        let sample_factor = (sample_count as f64 / self.requirements.ideal_samples as f64).min(1.0);
        let diversity_factor = diversity;
        let balance_factor = {
            if class_counts.is_empty() {
                0.0
            } else {
                let counts: Vec<_> = class_counts.values().copied().collect();
                let min = counts.iter().min().copied().unwrap_or(0) as f64;
                let max = counts.iter().max().copied().unwrap_or(1) as f64;
                if max == 0.0 { 0.0 } else { min / max }
            }
        };

        // Weighted combination
        (sample_factor * 0.4 + diversity_factor * 0.35 + balance_factor * 0.25).min(0.95)
    }
}

impl Default for DataSufficiencyAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
