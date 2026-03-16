//! Paradigm Shift Orchestrator implementation

use super::*;
use crate::models::{ShiftRecord, ShiftResult};
use chrono::Utc;
use uuid::Uuid;

/// Paradigm Shift Orchestrator
pub struct ParadigmShiftOrchestrator {
    detector: AnomalyDetector,
    sandbox: LlmSandbox,
    current_framework: String,
    pending_framework: Option<String>,
    _ab_test_id: Option<String>,
    state: ShiftState,
    history: Vec<ShiftRecord>,
}

impl ParadigmShiftOrchestrator {
    pub fn new(detector: AnomalyDetector, sandbox: LlmSandbox, current_framework: String) -> Self {
        Self {
            detector,
            sandbox,
            current_framework,
            pending_framework: None,
            _ab_test_id: None,
            state: ShiftState::Normal,
            history: Vec::new(),
        }
    }

    /// Tick the orchestrator - called periodically
    pub fn tick(&mut self) -> ShiftAction {
        match self.state {
            ShiftState::Normal => {
                let result = self.detector.detect();
                if result.recommendation == ShiftRecommendation::PrepareShift
                    || result.recommendation == ShiftRecommendation::ExecuteShift
                {
                    self.state = ShiftState::Detecting;
                    ShiftAction::StartInvestigation(result)
                } else {
                    ShiftAction::None
                }
            }
            ShiftState::Detecting => {
                // Create sandbox experiment with new framework variant
                let variant = self.create_alternative_framework();
                match self
                    .sandbox
                    .create_experiment("paradigm_shift_candidate", variant)
                {
                    Ok(exp_id) => {
                        self.pending_framework = Some(exp_id.clone());
                        self.state = ShiftState::Experimenting;
                        ShiftAction::StartExperiment(exp_id)
                    }
                    Err(e) => {
                        self.state = ShiftState::Normal;
                        ShiftAction::Error(e)
                    }
                }
            }
            ShiftState::Experimenting => {
                // Check if experiment is complete
                if let Some(ref exp_id) = self.pending_framework {
                    if let Some(exp) = self.sandbox.get_experiment(exp_id) {
                        match exp.status {
                            ExperimentStatus::Completed => {
                                self.state = ShiftState::Evaluating;
                                ShiftAction::EvaluateExperiment(exp_id.clone())
                            }
                            ExperimentStatus::Failed => {
                                self.state = ShiftState::Normal;
                                self.pending_framework = None;
                                ShiftAction::AbortExperiment
                            }
                            _ => ShiftAction::Wait,
                        }
                    } else {
                        self.state = ShiftState::Normal;
                        ShiftAction::Error("Experiment not found".to_string())
                    }
                } else {
                    self.state = ShiftState::Normal;
                    ShiftAction::Error("No pending framework".to_string())
                }
            }
            ShiftState::Evaluating => {
                // Evaluate if we should switch
                if let Some(ref exp_id) = self.pending_framework {
                    if let Some(exp) = self.sandbox.get_experiment(exp_id) {
                        if let Some(ref results) = exp.results {
                            let should_switch = self.evaluate_switch(results);
                            if should_switch {
                                self.state = ShiftState::Switching;
                                ShiftAction::ExecuteSwitch(exp_id.clone())
                            } else {
                                self.state = ShiftState::Normal;
                                self.pending_framework = None;
                                ShiftAction::AbortExperiment
                            }
                        } else {
                            self.state = ShiftState::Normal;
                            ShiftAction::Error("No experiment results".to_string())
                        }
                    } else {
                        self.state = ShiftState::Normal;
                        ShiftAction::Error("Experiment not found".to_string())
                    }
                } else {
                    self.state = ShiftState::Normal;
                    ShiftAction::Error("No pending framework".to_string())
                }
            }
            ShiftState::Switching => {
                // Complete the switch
                if let Some(exp_id) = self.pending_framework.clone() {
                    self.record_shift(ShiftResult::Success, exp_id.clone());
                    self.current_framework = exp_id;
                    self.pending_framework = None;
                    self.state = ShiftState::Normal;
                    ShiftAction::SwitchComplete
                } else {
                    self.state = ShiftState::Normal;
                    ShiftAction::Error("No pending framework during switch".to_string())
                }
            }
            ShiftState::RollingBack => {
                // Rollback to previous framework
                self.record_shift(ShiftResult::Rollback, self.current_framework.clone());
                self.pending_framework = None;
                self.state = ShiftState::Normal;
                ShiftAction::RollbackComplete
            }
        }
    }

    /// Create an alternative framework variant for testing
    fn create_alternative_framework(&self) -> FrameworkVariant {
        FrameworkVariant {
            id: Uuid::new_v4().to_string(),
            name: "alternative_framework".to_string(),
            prompt_template: "alternative_prompt_v2".to_string(),
            aggregation_strategy: AggregationStrategy::Neural,
            conflict_resolution: ConflictResolutionStrategy::ConfidenceWeighted,
        }
    }

    /// Evaluate if we should switch to the new framework
    fn evaluate_switch(&self, results: &ExperimentResults) -> bool {
        // Minimum criteria for switching
        results.prediction_accuracy > 0.7
            && results.conflict_resolution_rate > 0.8
            && results.sample_size >= 100
    }

    /// Record a shift in history
    fn record_shift(&mut self, result: ShiftResult, new_framework: String) {
        let record = ShiftRecord {
            timestamp: Utc::now(),
            result,
            old_framework_hash: self.current_framework.clone(),
            new_framework_hash: Some(new_framework),
            improvement_pct: None,
        };
        self.history.push(record);
    }

    /// Trigger manual rollback
    pub fn rollback(&mut self) {
        self.state = ShiftState::RollingBack;
    }

    /// Get current state
    pub fn state(&self) -> ShiftState {
        self.state
    }

    /// Get shift history
    pub fn history(&self) -> &[ShiftRecord] {
        &self.history
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anomaly_detector_empty() {
        let detector = AnomalyDetector::new(AnomalyConfig::default());
        let result = detector.detect();
        assert!(!result.is_anomaly);
        assert_eq!(result.recommendation, ShiftRecommendation::Continue);
    }

    #[test]
    fn test_anomaly_detector_with_signals() {
        let mut detector = AnomalyDetector::new(AnomalyConfig::default());

        // Add normal snapshots
        for _ in 0..50 {
            detector.record_snapshot(SignalSnapshot {
                timestamp: Utc::now(),
                prediction_error_rate: 0.1,
                conflict_density: 0.1,
                cache_hit_rate: 0.8,
                avg_prediction_error: 0.1,
                insight_prediction_accuracy: 0.8,
                total_claims: 1000,
                active_conflicts: 10,
            });
        }

        // Add anomalous snapshots
        for _ in 0..50 {
            detector.record_snapshot(SignalSnapshot {
                timestamp: Utc::now(),
                prediction_error_rate: 0.6,
                conflict_density: 0.4,
                cache_hit_rate: 0.5,
                avg_prediction_error: 0.5,
                insight_prediction_accuracy: 0.5,
                total_claims: 1000,
                active_conflicts: 50,
            });
        }

        let result = detector.detect();
        assert!(result.is_anomaly);
        assert!(!result.signals.is_empty());
    }

    #[test]
    fn test_llm_sandbox() {
        let config = SandboxConfig {
            max_experiments: 5,
            max_duration_minutes: 60,
            isolation_level: IsolationLevel::Process,
            resource_limits: ResourceLimits {
                max_tokens: 100000,
                max_requests: 1000,
                memory_mb: 512,
            },
        };

        let mut sandbox = LlmSandbox::new(config);

        let variant = FrameworkVariant {
            id: "test".to_string(),
            name: "Test Framework".to_string(),
            prompt_template: "test_prompt".to_string(),
            aggregation_strategy: AggregationStrategy::Bayesian,
            conflict_resolution: ConflictResolutionStrategy::TemporalPriority,
        };

        let exp_id = sandbox.create_experiment("test_exp", variant).unwrap();
        assert_eq!(sandbox.list_experiments().len(), 1);

        sandbox.start_experiment(&exp_id).unwrap();

        let results = ExperimentResults {
            prediction_accuracy: 0.85,
            conflict_resolution_rate: 0.9,
            avg_confidence: 0.8,
            processing_time_ms: 100,
            sample_size: 200,
        };

        sandbox.complete_experiment(&exp_id, results).unwrap();

        let exp = sandbox.get_experiment(&exp_id).unwrap();
        assert_eq!(exp.status, ExperimentStatus::Completed);
        assert!(exp.results.is_some());
    }

    #[test]
    fn test_paradigm_shift_orchestrator() {
        let detector = AnomalyDetector::new(AnomalyConfig::default());
        let sandbox = LlmSandbox::new(SandboxConfig {
            max_experiments: 5,
            max_duration_minutes: 60,
            isolation_level: IsolationLevel::Process,
            resource_limits: ResourceLimits {
                max_tokens: 100000,
                max_requests: 1000,
                memory_mb: 512,
            },
        });

        let mut orchestrator =
            ParadigmShiftOrchestrator::new(detector, sandbox, "current".to_string());

        assert_eq!(orchestrator.state(), ShiftState::Normal);

        // Initially should return None
        let action = orchestrator.tick();
        assert!(matches!(action, ShiftAction::None));
    }
}
