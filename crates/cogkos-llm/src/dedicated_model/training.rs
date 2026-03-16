//! Model training, switching, and system management

use super::analyzer::*;
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use uuid::Uuid;

/// Training configuration
#[derive(Debug, Clone)]
pub struct TrainingConfig {
    pub epochs: u32,
    pub batch_size: u32,
    pub learning_rate: f64,
    pub validation_split: f64,
    pub early_stopping_patience: u32,
}

impl Default for TrainingConfig {
    fn default() -> Self {
        Self {
            epochs: 10,
            batch_size: 32,
            learning_rate: 0.001,
            validation_split: 0.2,
            early_stopping_patience: 3,
        }
    }
}

/// Model trainer
pub struct ModelTrainer {
    /// Default architecture for new models
    default_architecture: ModelArchitecture,
    /// Training configuration
    config: TrainingConfig,
}

impl ModelTrainer {
    pub fn new() -> Self {
        Self {
            default_architecture: ModelArchitecture::FineTunedSmall,
            config: TrainingConfig::default(),
        }
    }

    pub fn with_architecture(mut self, arch: ModelArchitecture) -> Self {
        self.default_architecture = arch;
        self
    }

    /// Train a new model
    pub fn train(
        &self,
        domain: &str,
        samples: &[TrainingSample],
    ) -> Result<DedicatedPredictionModel> {
        // Check data sufficiency
        let analyzer = DataSufficiencyAnalyzer::new();
        let sufficiency = analyzer.analyze(samples);

        if !sufficiency.is_sufficient {
            return Err(DedicatedModelError::InsufficientData(format!(
                "Data insufficient: {:?}",
                sufficiency.gaps
            )));
        }

        let model_id = format!("{}-{}", domain, Uuid::new_v4());
        let now = Utc::now();

        // Simulate training
        let training_info = TrainingInfo {
            started_at: now,
            completed_at: Some(now + Duration::minutes(30)),
            sample_count: samples.len(),
            training_duration_seconds: 1800,
            data_hash: format!("hash_{}", samples.len()),
            validation_split: self.config.validation_split,
        };

        // Simulate performance based on data quality
        let base_performance = sufficiency.estimated_performance;
        let performance = ModelPerformance {
            accuracy: base_performance * 0.9 + 0.05,
            precision: base_performance * 0.88 + 0.06,
            recall: base_performance * 0.87 + 0.07,
            f1_score: base_performance * 0.875 + 0.065,
            confidence_calibration: 0.8,
            latency_ms: match self.default_architecture {
                ModelArchitecture::FineTunedSmall => 50,
                ModelArchitecture::FineTunedMedium => 100,
                ModelArchitecture::FineTunedLarge => 200,
                ModelArchitecture::LoraAdapter => 75,
                ModelArchitecture::PromptTuned => 60,
                ModelArchitecture::RagEnhanced => 150,
            },
            evaluated_at: Utc::now(),
        };

        Ok(DedicatedPredictionModel {
            model_id,
            domain: domain.to_string(),
            architecture: self.default_architecture,
            status: ModelStatus::Ready,
            version: 1,
            training_info,
            performance,
            created_at: now,
            updated_at: now,
            hyperparameters: {
                let mut params = HashMap::new();
                params.insert("learning_rate".to_string(), self.config.learning_rate);
                params.insert("epochs".to_string(), self.config.epochs as f64);
                params
            },
        })
    }

    /// Retrain existing model with new data
    pub fn retrain(
        &self,
        existing_model: &DedicatedPredictionModel,
        new_samples: &[TrainingSample],
    ) -> Result<DedicatedPredictionModel> {
        let mut model = existing_model.clone();
        model.version += 1;
        model.status = ModelStatus::Training;

        // Simulate retraining
        model.training_info.started_at = Utc::now();
        model.training_info.completed_at = Some(Utc::now() + Duration::minutes(20));
        model.training_info.sample_count = new_samples.len();
        model.updated_at = Utc::now();
        model.status = ModelStatus::Ready;

        // Slightly improve performance
        model.performance.accuracy = (model.performance.accuracy * 1.02).min(0.95);
        model.performance.evaluated_at = Utc::now();

        Ok(model)
    }
}

impl Default for ModelTrainer {
    fn default() -> Self {
        Self::new()
    }
}

/// Model switch configuration
#[derive(Debug, Clone)]
pub struct ModelSwitchConfig {
    /// Minimum improvement required to switch
    pub min_improvement_threshold: f64,
    /// Require statistical significance
    pub require_significance: bool,
    /// Graceful transition period
    pub transition_period_minutes: i64,
    /// Rollback on degradation
    pub enable_auto_rollback: bool,
    /// Degradation threshold for rollback
    pub rollback_threshold: f64,
}

impl Default for ModelSwitchConfig {
    fn default() -> Self {
        Self {
            min_improvement_threshold: 0.05, // 5%
            require_significance: true,
            transition_period_minutes: 30,
            enable_auto_rollback: true,
            rollback_threshold: 0.1, // 10% degradation
        }
    }
}

/// Model switch decision
#[derive(Debug, Clone)]
pub struct ModelSwitchDecision {
    pub should_switch: bool,
    pub from_model: String,
    pub to_model: String,
    pub improvement: f64,
    pub confidence: f64,
    pub risk_level: SwitchRiskLevel,
    pub recommendation: SwitchRecommendation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchRiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchRecommendation {
    SwitchImmediately,
    GradualTransition,
    AwaitMoreData,
    DoNotSwitch,
    Rollback,
}

/// Model switch manager
pub struct ModelSwitchManager {
    config: ModelSwitchConfig,
    /// Currently active model per domain
    active_models: HashMap<String, String>,
    /// Model performance history
    _performance_history: HashMap<String, Vec<ModelPerformance>>,
    /// Pending switches
    pending_switches: Vec<PendingSwitch>,
}

#[derive(Debug, Clone)]
pub struct PendingSwitch {
    pub domain: String,
    pub from_model: String,
    pub to_model: String,
    pub initiated_at: DateTime<Utc>,
    pub completion_at: DateTime<Utc>,
    pub status: SwitchStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchStatus {
    Pending,
    InProgress,
    Completed,
    RolledBack,
}

impl ModelSwitchManager {
    pub fn new() -> Self {
        Self {
            config: ModelSwitchConfig::default(),
            active_models: HashMap::new(),
            _performance_history: HashMap::new(),
            pending_switches: Vec::new(),
        }
    }

    pub fn with_config(mut self, config: ModelSwitchConfig) -> Self {
        self.config = config;
        self
    }

    /// Register an active model for a domain
    pub fn register_active_model(&mut self, domain: &str, model_id: &str) {
        self.active_models
            .insert(domain.to_string(), model_id.to_string());
    }

    /// Evaluate whether to switch models
    pub fn evaluate_switch(
        &self,
        current_model: &DedicatedPredictionModel,
        candidate_model: &DedicatedPredictionModel,
    ) -> ModelSwitchDecision {
        let improvement = candidate_model.performance.accuracy - current_model.performance.accuracy;
        let relative_improvement = if current_model.performance.accuracy > 0.0 {
            improvement / current_model.performance.accuracy
        } else {
            0.0
        };

        // Determine if improvement is significant
        let is_significant = relative_improvement >= self.config.min_improvement_threshold;

        // Calculate confidence based on sample sizes and consistency
        let confidence = (candidate_model.training_info.sample_count as f64 / 1000.0).min(1.0)
            * 0.7
            + candidate_model.performance.confidence_calibration * 0.3;

        // Determine risk level
        let risk_level = if relative_improvement > 0.2 {
            SwitchRiskLevel::High // Large jump is risky
        } else if relative_improvement > 0.1 {
            SwitchRiskLevel::Medium
        } else {
            SwitchRiskLevel::Low
        };

        // Make recommendation
        let (should_switch, recommendation) = if !is_significant {
            (false, SwitchRecommendation::DoNotSwitch)
        } else if self.config.require_significance && confidence < 0.7 {
            (false, SwitchRecommendation::AwaitMoreData)
        } else if risk_level == SwitchRiskLevel::High {
            (true, SwitchRecommendation::GradualTransition)
        } else {
            (true, SwitchRecommendation::SwitchImmediately)
        };

        ModelSwitchDecision {
            should_switch,
            from_model: current_model.model_id.clone(),
            to_model: candidate_model.model_id.clone(),
            improvement: relative_improvement,
            confidence,
            risk_level,
            recommendation,
        }
    }

    /// Execute a model switch
    pub fn execute_switch(
        &mut self,
        domain: &str,
        from_model: &str,
        to_model: &str,
    ) -> Result<PendingSwitch> {
        let now = Utc::now();
        let completion = now + Duration::minutes(self.config.transition_period_minutes);

        let pending = PendingSwitch {
            domain: domain.to_string(),
            from_model: from_model.to_string(),
            to_model: to_model.to_string(),
            initiated_at: now,
            completion_at: completion,
            status: SwitchStatus::Pending,
        };

        self.pending_switches.push(pending.clone());

        // Update active model
        self.active_models
            .insert(domain.to_string(), to_model.to_string());

        Ok(pending)
    }

    /// Rollback to previous model
    pub fn rollback(&mut self, domain: &str) -> Result<()> {
        let current = self
            .active_models
            .get(domain)
            .ok_or_else(|| DedicatedModelError::SwitchFailed("No active model".to_string()))?;

        // Find the pending switch for this domain
        if let Some(pending) = self
            .pending_switches
            .iter_mut()
            .find(|s| s.domain == domain && s.to_model == *current)
        {
            pending.status = SwitchStatus::RolledBack;
            self.active_models
                .insert(domain.to_string(), pending.from_model.clone());
            Ok(())
        } else {
            Err(DedicatedModelError::SwitchFailed(
                "No switch to rollback".to_string(),
            ))
        }
    }

    /// Get active model for domain
    pub fn get_active_model(&self, domain: &str) -> Option<&String> {
        self.active_models.get(domain)
    }

    /// Check if should auto-rollback based on performance
    pub fn check_auto_rollback(
        &self,
        current_performance: &ModelPerformance,
        baseline_performance: &ModelPerformance,
    ) -> bool {
        if !self.config.enable_auto_rollback {
            return false;
        }

        let degradation = baseline_performance.accuracy - current_performance.accuracy;
        let relative_degradation = if baseline_performance.accuracy > 0.0 {
            degradation / baseline_performance.accuracy
        } else {
            0.0
        };

        relative_degradation > self.config.rollback_threshold
    }
}

impl Default for ModelSwitchManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Complete dedicated model system
pub struct DedicatedModelSystem {
    pub analyzer: DataSufficiencyAnalyzer,
    pub trainer: ModelTrainer,
    pub switch_manager: ModelSwitchManager,
    pub models: HashMap<String, DedicatedPredictionModel>,
}

impl DedicatedModelSystem {
    pub fn new() -> Self {
        Self {
            analyzer: DataSufficiencyAnalyzer::new(),
            trainer: ModelTrainer::new(),
            switch_manager: ModelSwitchManager::new(),
            models: HashMap::new(),
        }
    }

    /// Analyze data sufficiency for a domain
    pub fn analyze_data(&self, samples: &[TrainingSample]) -> DataSufficiencyResult {
        self.analyzer.analyze(samples)
    }

    /// Train a new model for a domain
    pub fn train_model(
        &mut self,
        domain: &str,
        samples: &[TrainingSample],
    ) -> Result<DedicatedPredictionModel> {
        let model = self.trainer.train(domain, samples)?;
        self.models.insert(model.model_id.clone(), model.clone());
        Ok(model)
    }

    /// Evaluate and potentially switch models
    pub fn evaluate_and_switch(
        &mut self,
        domain: &str,
        current_model_id: &str,
        candidate_model: &DedicatedPredictionModel,
    ) -> Option<ModelSwitchDecision> {
        let current = self.models.get(current_model_id)?;
        let decision = self
            .switch_manager
            .evaluate_switch(current, candidate_model);

        if decision.should_switch {
            self.switch_manager
                .execute_switch(domain, current_model_id, &candidate_model.model_id)
                .ok()?;
        }

        Some(decision)
    }

    /// Get active model for domain
    pub fn get_active_model(&self, domain: &str) -> Option<&DedicatedPredictionModel> {
        let model_id = self.switch_manager.get_active_model(domain)?;
        self.models.get(model_id)
    }
}

impl Default for DedicatedModelSystem {
    fn default() -> Self {
        Self::new()
    }
}
