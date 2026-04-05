//! LLM Sandbox for safe paradigm experimentation

use super::*;
use chrono::Utc;
use std::collections::HashMap;
use uuid::Uuid;

/// LLM Sandbox for safe paradigm experimentation
#[derive(Debug, Clone)]
pub struct LlmSandbox {
    config: SandboxConfig,
    experiments: HashMap<String, SandboxExperiment>,
}

impl LlmSandbox {
    pub fn new(config: SandboxConfig) -> Self {
        Self {
            config,
            experiments: HashMap::new(),
        }
    }

    /// Create a new experiment
    pub fn create_experiment(
        &mut self,
        name: &str,
        variant: FrameworkVariant,
    ) -> Result<String, String> {
        if self.experiments.len() >= self.config.max_experiments {
            return Err("Maximum number of experiments reached".to_string());
        }

        let id = Uuid::new_v4().to_string();
        let experiment = SandboxExperiment {
            id: id.clone(),
            name: name.to_string(),
            framework_variant: variant,
            status: ExperimentStatus::Pending,
            started_at: Utc::now(),
            ended_at: None,
            results: None,
            resource_usage: ResourceUsage::default(),
        };

        self.experiments.insert(id.clone(), experiment);
        Ok(id)
    }

    /// Start an experiment
    pub fn start_experiment(&mut self, experiment_id: &str) -> Result<(), String> {
        let experiment = self
            .experiments
            .get_mut(experiment_id)
            .ok_or("Experiment not found")?;

        if experiment.status != ExperimentStatus::Pending {
            return Err("Experiment is not in pending state".to_string());
        }

        experiment.status = ExperimentStatus::Running;
        experiment.started_at = Utc::now();
        Ok(())
    }

    /// Complete an experiment with results
    pub fn complete_experiment(
        &mut self,
        experiment_id: &str,
        results: ExperimentResults,
    ) -> Result<(), String> {
        let experiment = self
            .experiments
            .get_mut(experiment_id)
            .ok_or("Experiment not found")?;

        if experiment.status != ExperimentStatus::Running {
            return Err("Experiment is not running".to_string());
        }

        experiment.status = ExperimentStatus::Completed;
        experiment.ended_at = Some(Utc::now());
        experiment.results = Some(results);
        Ok(())
    }

    /// Fail an experiment
    pub fn fail_experiment(&mut self, experiment_id: &str, reason: &str) -> Result<(), String> {
        let experiment = self
            .experiments
            .get_mut(experiment_id)
            .ok_or("Experiment not found")?;

        experiment.status = ExperimentStatus::Failed;
        experiment.ended_at = Some(Utc::now());
        // Could store failure reason in metadata
        let _ = reason;
        Ok(())
    }

    /// Get experiment results
    pub fn get_experiment(&self, experiment_id: &str) -> Option<&SandboxExperiment> {
        self.experiments.get(experiment_id)
    }

    /// List all experiments
    pub fn list_experiments(&self) -> Vec<&SandboxExperiment> {
        self.experiments.values().collect()
    }

    /// Get best performing experiment
    pub fn get_best_experiment(&self) -> Option<&SandboxExperiment> {
        self.experiments
            .values()
            .filter(|e| e.status == ExperimentStatus::Completed)
            .filter_map(|e| e.results.as_ref().map(|r| (e, r.prediction_accuracy)))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(e, _)| e)
    }

    /// Check if resource limits exceeded
    pub fn check_resource_limits(&self, experiment_id: &str) -> bool {
        if let Some(exp) = self.experiments.get(experiment_id) {
            exp.resource_usage.tokens_used > self.config.resource_limits.max_tokens
                || exp.resource_usage.requests_made > self.config.resource_limits.max_requests
                || exp.resource_usage.memory_mb > self.config.resource_limits.memory_mb
        } else {
            false
        }
    }

    /// Update resource usage
    pub fn record_resource_usage(&mut self, experiment_id: &str, tokens: u64, requests: u32) {
        if let Some(exp) = self.experiments.get_mut(experiment_id) {
            exp.resource_usage.tokens_used += tokens;
            exp.resource_usage.requests_made += requests;
        }
    }
}
