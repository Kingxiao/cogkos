//! DedicatedModelManager implementation

use super::*;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use uuid::Uuid;

/// Dedicated prediction model manager
pub struct DedicatedModelManager {
    config: PredictionModelConfig,
    /// Domain-specific models
    domain_models: HashMap<String, DomainModel>,
    /// Fallback model (LLM-based)
    fallback_model: ModelMetadata,
    /// Currently active models per domain
    active_models: HashMap<String, String>,
    /// Training queue
    training_queue: Vec<TrainingJob>,
    /// Model performance history
    performance_history: Vec<ModelPerformanceRecord>,
}

impl DedicatedModelManager {
    pub fn new(config: PredictionModelConfig) -> Self {
        Self {
            config: config.clone(),
            domain_models: HashMap::new(),
            fallback_model: ModelMetadata {
                model_id: "llm_fallback".to_string(),
                model_type: ModelType::Neural,
                description: "LLM-based fallback prediction".to_string(),
            },
            active_models: HashMap::new(),
            training_queue: Vec::new(),
            performance_history: Vec::new(),
        }
    }

    /// Check if data is sufficient for training a domain model
    pub fn check_data_sufficiency(
        &self,
        _domain: &str,
        samples: &[TrainingSample],
    ) -> DataSufficiencyResult {
        let sample_count = samples.len();
        let required_samples = self.config.min_samples_for_training;

        // Calculate quality metrics
        let quality_score = self.calculate_data_quality(samples);

        // Check class balance
        let class_balance = self.calculate_class_balance(samples);

        // Generate recommendations
        let mut recommendations = Vec::new();

        if sample_count < required_samples {
            recommendations.push(DataRecommendation {
                recommendation_type: DataRecType::CollectMoreData,
                description: format!(
                    "Need {} more samples for reliable training",
                    required_samples - sample_count
                ),
                impact: ImpactLevel::Critical,
            });
        }

        if class_balance < 0.3 {
            recommendations.push(DataRecommendation {
                recommendation_type: DataRecType::BalanceClasses,
                description: "Class distribution is imbalanced".to_string(),
                impact: ImpactLevel::High,
            });
        }

        // Estimate potential accuracy based on data characteristics
        let estimated_accuracy = if sample_count >= required_samples {
            let base_accuracy = 0.7;
            let data_bonus = (sample_count as f64 / required_samples as f64).min(2.0) * 0.1;
            let quality_bonus = quality_score * 0.1;
            let balance_penalty = (1.0 - class_balance) * 0.1;
            Some((base_accuracy + data_bonus + quality_bonus - balance_penalty).min(0.95))
        } else {
            None
        };

        DataSufficiencyResult {
            is_sufficient: sample_count >= required_samples
                && quality_score > 0.5
                && class_balance > 0.2,
            sample_count,
            required_samples,
            quality_score,
            recommendations,
            estimated_accuracy,
        }
    }

    /// Calculate data quality score
    fn calculate_data_quality(&self, samples: &[TrainingSample]) -> f64 {
        if samples.is_empty() {
            return 0.0;
        }

        // Check for missing values
        let completeness: f64 = samples
            .iter()
            .map(|s| {
                let total = s.features.len();
                let present = s.features.values().filter(|v| !self.is_missing(v)).count();
                present as f64 / total as f64
            })
            .sum::<f64>()
            / samples.len() as f64;

        // Check feature variance
        let variance = self.calculate_feature_variance(samples);

        // Combine metrics
        (completeness * 0.6 + variance * 0.4).clamp(0.0, 1.0)
    }

    fn is_missing(&self, value: &FeatureValue) -> bool {
        matches!(value, FeatureValue::Categorical(s) if s.is_empty() || s == "null" || s == "N/A")
    }

    fn calculate_feature_variance(&self, samples: &[TrainingSample]) -> f64 {
        // Simplified variance calculation
        if samples.len() < 2 {
            return 0.0;
        }

        // Calculate variance for numeric features
        let numeric_variances: Vec<f64> = samples[0]
            .features
            .keys()
            .filter_map(|key| {
                let values: Vec<f64> = samples
                    .iter()
                    .filter_map(|s| match s.features.get(key) {
                        Some(FeatureValue::Numeric(v)) => Some(*v),
                        _ => None,
                    })
                    .collect();

                if values.len() >= 2 {
                    let mean = values.iter().sum::<f64>() / values.len() as f64;
                    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                        / values.len() as f64;
                    Some(variance)
                } else {
                    None
                }
            })
            .collect();

        if numeric_variances.is_empty() {
            return 0.5; // Default for non-numeric features
        }

        let avg_variance = numeric_variances.iter().sum::<f64>() / numeric_variances.len() as f64;
        avg_variance.min(1.0) // Normalize to 0-1
    }

    /// Calculate class balance
    fn calculate_class_balance(&self, samples: &[TrainingSample]) -> f64 {
        if samples.is_empty() {
            return 0.0;
        }

        let mut class_counts: HashMap<String, u32> = HashMap::new();
        for sample in samples {
            *class_counts.entry(sample.label.clone()).or_insert(0) += 1;
        }

        if class_counts.len() < 2 {
            return 0.0;
        }

        let max_count = *class_counts.values().max().unwrap_or(&1);
        let min_count = *class_counts.values().min().unwrap_or(&1);

        min_count as f64 / max_count as f64
    }

    /// Queue a model training job
    pub fn queue_training(
        &mut self,
        domain: &str,
        samples: Vec<TrainingSample>,
    ) -> Result<String, String> {
        // Check domain limit
        if self.domain_models.len() >= self.config.max_domain_models
            && !self.domain_models.contains_key(domain)
        {
            return Err("Maximum number of domain models reached".to_string());
        }

        // Check data sufficiency
        let sufficiency = self.check_data_sufficiency(domain, &samples);
        if !sufficiency.is_sufficient {
            return Err(format!(
                "Insufficient data for training: {:?}",
                sufficiency.recommendations
            ));
        }

        let job_id = Uuid::new_v4().to_string();
        let job = TrainingJob {
            job_id: job_id.clone(),
            domain: domain.to_string(),
            status: TrainingStatus::Queued,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            data_stats: DataStats {
                available_samples: samples.len(),
                unique_features: samples.first().map(|s| s.features.len()).unwrap_or(0),
                class_balance: self.calculate_class_balance(&samples),
            },
        };

        self.training_queue.push(job);
        Ok(job_id)
    }

    /// Start training a model (simulated)
    pub fn start_training(&mut self, job_id: &str) -> Result<(), String> {
        if let Some(job) = self.training_queue.iter_mut().find(|j| j.job_id == job_id) {
            if job.status != TrainingStatus::Queued {
                return Err("Job is not in queued state".to_string());
            }

            job.status = TrainingStatus::Running;
            job.started_at = Some(Utc::now());
            Ok(())
        } else {
            Err("Job not found".to_string())
        }
    }

    /// Complete training and create model
    pub fn complete_training(
        &mut self,
        job_id: &str,
        performance: ModelPerformance,
    ) -> Result<String, String> {
        // Find the job and extract data first
        let job_data: Option<(String, DateTime<Utc>, usize, usize)> = {
            let job = self.training_queue.iter_mut().find(|j| j.job_id == job_id);
            if let Some(job) = job {
                if job.status != TrainingStatus::Running {
                    return Err("Job is not running".to_string());
                }
                Some((
                    job.domain.clone(),
                    job.created_at,
                    job.data_stats.available_samples,
                    job.data_stats.unique_features,
                ))
            } else {
                None
            }
        };

        if let Some((domain, job_created_at, available_samples, unique_features)) = job_data {
            // Now mark job as completed
            if let Some(job) = self.training_queue.iter_mut().find(|j| j.job_id == job_id) {
                job.status = TrainingStatus::Completed;
                job.completed_at = Some(Utc::now());
            }

            // Now we can call self methods
            let next_version = self.get_next_version(&domain);

            // Create domain model
            let model_id = format!("{}_v{}", domain, next_version);
            let model = DomainModel {
                model_id: model_id.clone(),
                domain: domain.clone(),
                version: next_version,
                status: ModelStatus::Ready,
                training_data: TrainingDataInfo {
                    sample_count: available_samples,
                    feature_count: unique_features,
                    label_distribution: HashMap::new(),
                    time_range: (job_created_at, Utc::now()),
                    quality_score: 0.8, // Would calculate from actual data
                },
                performance: performance.clone(),
                created_at: Utc::now(),
                last_trained_at: Utc::now(),
                model_type: ModelType::Ensemble,
                hyperparameters: HashMap::new(),
            };

            self.domain_models.insert(domain.clone(), model);

            // If this is the first model for this domain, activate it
            if !self.active_models.contains_key(&domain) {
                self.active_models.insert(domain.clone(), model_id.clone());
            }

            // Record performance
            self.performance_history.push(ModelPerformanceRecord {
                timestamp: Utc::now(),
                model_id: model_id.clone(),
                domain: domain.clone(),
                accuracy: performance.accuracy,
                sample_count: 0,
            });

            Ok(model_id)
        } else {
            Err("Job not found".to_string())
        }
    }

    fn get_next_version(&self, domain: &str) -> u32 {
        self.domain_models
            .get(domain)
            .map(|m| m.version + 1)
            .unwrap_or(1)
    }

    /// Deploy a model for a domain
    pub fn deploy_model(&mut self, domain: &str, model_id: &str) -> Result<(), String> {
        // Update domain model status
        if let Some(model) = self.domain_models.get_mut(domain) {
            if model.model_id != model_id {
                return Err("Model ID mismatch".to_string());
            }
            model.status = ModelStatus::Deployed;
        } else {
            return Err("Domain model not found".to_string());
        }

        // Activate the model
        self.active_models
            .insert(domain.to_string(), model_id.to_string());
        Ok(())
    }

    /// Make a prediction using the appropriate model
    pub fn predict(
        &self,
        context: &PredictionContext,
        features: &FeatureVector,
    ) -> DedicatedPrediction {
        // Check if we have a dedicated model for this domain
        if let Some(_model_id) = self.active_models.get(&context.domain)
            && let Some(model) = self.domain_models.get(&context.domain)
            && model.status == ModelStatus::Deployed
        {
            return self.predict_with_model(model, features);
        }

        // Fall back to LLM-based prediction
        self.predict_with_fallback(features)
    }

    fn predict_with_model(
        &self,
        model: &DomainModel,
        _features: &FeatureVector,
    ) -> DedicatedPrediction {
        // Simulated prediction
        DedicatedPrediction {
            model_id: model.model_id.clone(),
            prediction: "predicted_value".to_string(),
            confidence: model.performance.confidence_calibration,
            alternative_predictions: vec![],
            inference_time_ms: model.performance.inference_time_ms,
            feature_importance: HashMap::new(),
        }
    }

    fn predict_with_fallback(&self, _features: &FeatureVector) -> DedicatedPrediction {
        DedicatedPrediction {
            model_id: self.fallback_model.model_id.clone(),
            prediction: "llm_prediction".to_string(),
            confidence: 0.6,
            alternative_predictions: vec![],
            inference_time_ms: 500,
            feature_importance: HashMap::new(),
        }
    }

    /// Evaluate whether to switch models
    pub fn evaluate_model_switch(&self, domain: &str) -> ModelSwitchDecision {
        let current_model_id = match self.active_models.get(domain) {
            Some(id) => id,
            None => {
                return ModelSwitchDecision {
                    should_switch: false,
                    current_model: "none".to_string(),
                    proposed_model: "none".to_string(),
                    improvement: 0.0,
                    confidence: 0.0,
                    reason: "No active model for domain".to_string(),
                };
            }
        };

        let current_model = match self.domain_models.get(domain) {
            Some(m) => m,
            None => {
                return ModelSwitchDecision {
                    should_switch: false,
                    current_model: current_model_id.clone(),
                    proposed_model: "none".to_string(),
                    improvement: 0.0,
                    confidence: 0.0,
                    reason: "Current model not found".to_string(),
                };
            }
        };

        // Get recent performance data
        let recent_performance: Vec<_> = self
            .performance_history
            .iter()
            .filter(|p| p.domain == domain)
            .rev()
            .take(10)
            .collect();

        if recent_performance.len() < 5 {
            return ModelSwitchDecision {
                should_switch: false,
                current_model: current_model_id.clone(),
                proposed_model: "none".to_string(),
                improvement: 0.0,
                confidence: 0.0,
                reason: "Insufficient performance history".to_string(),
            };
        }

        let avg_recent_accuracy: f64 = recent_performance.iter().map(|p| p.accuracy).sum::<f64>()
            / recent_performance.len() as f64;

        // Check if there's a newer model available
        // For now, simplified logic
        let improvement = avg_recent_accuracy - current_model.performance.accuracy;

        if improvement > self.config.switch_improvement_threshold {
            ModelSwitchDecision {
                should_switch: self.config.enable_auto_switch,
                current_model: current_model_id.clone(),
                proposed_model: format!("{}_v{}", domain, current_model.version + 1),
                improvement,
                confidence: 0.7,
                reason: format!("New model shows {:.1}% improvement", improvement * 100.0),
            }
        } else {
            ModelSwitchDecision {
                should_switch: false,
                current_model: current_model_id.clone(),
                proposed_model: format!("{}_v{}", domain, current_model.version + 1),
                improvement,
                confidence: 0.5,
                reason: "Current model performance is adequate".to_string(),
            }
        }
    }

    /// Execute model switch
    pub fn switch_model(&mut self, domain: &str, new_model_id: &str) -> Result<(), String> {
        // Validate new model exists and is ready
        if let Some(model) = self.domain_models.get(domain) {
            if model.model_id != new_model_id {
                return Err("Model ID does not match domain".to_string());
            }
            if model.status != ModelStatus::Ready && model.status != ModelStatus::Deployed {
                return Err("Model is not ready for deployment".to_string());
            }
        } else {
            return Err("Model not found".to_string());
        }

        // Update active model
        self.active_models
            .insert(domain.to_string(), new_model_id.to_string());

        // Update model status
        if let Some(model) = self.domain_models.get_mut(domain) {
            model.status = ModelStatus::Deployed;
        }

        Ok(())
    }

    /// Get model status for a domain
    pub fn get_model_status(&self, domain: &str) -> Option<&DomainModel> {
        self.domain_models.get(domain)
    }

    /// Get active model for a domain
    pub fn get_active_model(&self, domain: &str) -> Option<&str> {
        self.active_models.get(domain).map(|s| s.as_str())
    }

    /// List all domains with models
    pub fn list_domains(&self) -> Vec<String> {
        self.domain_models.keys().cloned().collect()
    }

    /// Get training queue
    pub fn get_training_queue(&self) -> &[TrainingJob] {
        &self.training_queue
    }

    /// Check if model needs retraining
    pub fn needs_retraining(&self, domain: &str) -> bool {
        if let Some(model) = self.domain_models.get(domain) {
            let hours_since_training = (Utc::now() - model.last_trained_at).num_hours();
            hours_since_training > self.config.retraining_interval_hours
        } else {
            false
        }
    }

    /// Get performance history for a domain
    pub fn get_performance_history(&self, domain: &str) -> Vec<&ModelPerformanceRecord> {
        self.performance_history
            .iter()
            .filter(|p| p.domain == domain)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_sufficiency_insufficient() {
        let manager = DedicatedModelManager::new(PredictionModelConfig::default());
        let samples: Vec<TrainingSample> = (0..50)
            .map(|i| TrainingSample::new(if i % 2 == 0 { "A" } else { "B" }))
            .collect();

        let result = manager.check_data_sufficiency("test_domain", &samples);
        assert!(!result.is_sufficient);
        assert!(result.sample_count < result.required_samples);
    }

    #[test]
    fn test_data_sufficiency_sufficient() {
        let manager = DedicatedModelManager::new(PredictionModelConfig::default());
        let samples: Vec<TrainingSample> = (0..150)
            .map(|i| {
                TrainingSample::new(if i % 2 == 0 { "A" } else { "B" })
                    .with_feature("feature1", FeatureValue::Numeric(i as f64))
                    .with_feature("feature2", FeatureValue::Categorical("test".to_string()))
            })
            .collect();

        let result = manager.check_data_sufficiency("test_domain", &samples);
        assert!(result.is_sufficient);
        assert!(result.estimated_accuracy.is_some());
    }

    #[test]
    fn test_training_queue() {
        let mut manager = DedicatedModelManager::new(PredictionModelConfig::default());
        let samples: Vec<TrainingSample> = (0..150)
            .map(|i| {
                TrainingSample::new(if i % 2 == 0 { "A" } else { "B" })
                    .with_feature("feature1", FeatureValue::Numeric(i as f64))
                    .with_feature("feature2", FeatureValue::Categorical("test".to_string()))
            })
            .collect();

        let job_id = manager.queue_training("test_domain", samples).unwrap();
        assert_eq!(manager.get_training_queue().len(), 1);

        manager.start_training(&job_id).unwrap();
        let queue = manager.get_training_queue();
        assert_eq!(queue[0].status, TrainingStatus::Running);
    }

    #[test]
    fn test_model_switch() {
        let mut manager = DedicatedModelManager::new(PredictionModelConfig::default());

        let samples: Vec<TrainingSample> = (0..150)
            .map(|i| {
                TrainingSample::new(if i % 2 == 0 { "A" } else { "B" })
                    .with_feature("feature1", FeatureValue::Numeric(i as f64))
                    .with_feature("feature2", FeatureValue::Categorical("test".to_string()))
            })
            .collect();

        let job_id = manager.queue_training("test_domain", samples).unwrap();
        manager.start_training(&job_id).unwrap();

        let performance = ModelPerformance {
            accuracy: 0.85,
            precision: 0.84,
            recall: 0.86,
            f1_score: 0.85,
            training_time_ms: 1000,
            inference_time_ms: 10,
            validation_loss: 0.15,
            confidence_calibration: 0.9,
        };

        let model_id = manager.complete_training(&job_id, performance).unwrap();
        manager.deploy_model("test_domain", &model_id).unwrap();

        assert_eq!(
            manager.get_active_model("test_domain"),
            Some(model_id.as_str())
        );
    }

    #[test]
    fn test_predict_with_fallback() {
        let manager = DedicatedModelManager::new(PredictionModelConfig::default());

        let context = PredictionContext {
            domain: "unknown_domain".to_string(),
            tenant_id: "test".to_string(),
            related_claims: vec![],
            temporal_context: None,
        };

        let features = FeatureVector {
            features: HashMap::new(),
            context: context.clone(),
        };

        let prediction = manager.predict(&context, &features);
        assert_eq!(prediction.model_id, "llm_fallback");
    }

    #[test]
    fn test_needs_retraining() {
        let mut manager = DedicatedModelManager::new(PredictionModelConfig::default());

        let samples: Vec<TrainingSample> = (0..150)
            .map(|i| {
                TrainingSample::new(if i % 2 == 0 { "A" } else { "B" })
                    .with_feature("feature1", FeatureValue::Numeric(i as f64))
                    .with_feature("feature2", FeatureValue::Categorical("test".to_string()))
            })
            .collect();

        let job_id = manager.queue_training("test_domain", samples).unwrap();
        manager.start_training(&job_id).unwrap();

        let performance = ModelPerformance {
            accuracy: 0.85,
            ..Default::default()
        };

        manager.complete_training(&job_id, performance).unwrap();

        // Just trained, should not need retraining
        assert!(!manager.needs_retraining("test_domain"));
    }
}
