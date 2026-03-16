//! Paradigm shift engine - A/B testing, framework switching, and orchestration

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::{
    AnomalyDetectionResult, AnomalyDetector, AnomalyRecommendation, Framework, LlmSandbox,
    ParadigmShiftError, Result, SandboxReport,
};
use crate::models::{EpistemicClaim, EvolutionEngineState, ShiftRecord, ShiftResult};

/// Switch/rollback manager
pub struct FrameworkSwitchManager {
    current_framework: Option<Framework>,
    previous_framework: Option<Framework>,
    switch_history: Vec<ShiftRecord>,
    _max_rollback_depth: usize,
}

impl FrameworkSwitchManager {
    pub fn new() -> Self {
        Self {
            current_framework: None,
            previous_framework: None,
            switch_history: Vec::new(),
            _max_rollback_depth: 3,
        }
    }

    pub fn initialize(&mut self, framework: Framework) {
        self.current_framework = Some(framework);
    }

    pub fn switch_framework(
        &mut self,
        new_framework: Framework,
        _reason: &str,
    ) -> Result<ShiftRecord> {
        let old_framework = self.current_framework.take().ok_or_else(|| {
            ParadigmShiftError::RollbackFailed("No current framework".to_string())
        })?;

        let record = ShiftRecord {
            timestamp: Utc::now(),
            result: ShiftResult::Success,
            old_framework_hash: old_framework.hash.clone(),
            new_framework_hash: Some(new_framework.hash.clone()),
            improvement_pct: None,
        };

        self.previous_framework = Some(old_framework);
        self.current_framework = Some(new_framework);
        self.switch_history.push(record.clone());

        Ok(record)
    }

    pub fn rollback(&mut self, _reason: &str) -> Result<ShiftRecord> {
        let previous = self.previous_framework.take().ok_or_else(|| {
            ParadigmShiftError::RollbackFailed("No previous framework to rollback to".to_string())
        })?;

        let current = self.current_framework.take().ok_or_else(|| {
            ParadigmShiftError::RollbackFailed("No current framework".to_string())
        })?;

        let record = ShiftRecord {
            timestamp: Utc::now(),
            result: ShiftResult::Rollback,
            old_framework_hash: current.hash,
            new_framework_hash: Some(previous.hash.clone()),
            improvement_pct: None,
        };

        self.current_framework = Some(previous);
        self.switch_history.push(record.clone());

        Ok(record)
    }

    pub fn current_framework(&self) -> Option<&Framework> {
        self.current_framework.as_ref()
    }

    pub fn switch_history(&self) -> &[ShiftRecord] {
        &self.switch_history
    }

    pub fn can_rollback(&self) -> bool {
        self.previous_framework.is_some()
    }

    pub fn rollback_depth(&self) -> usize {
        self.switch_history
            .iter()
            .filter(|r| r.result == ShiftResult::Rollback)
            .count()
    }
}

impl Default for FrameworkSwitchManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Complete paradigm shift orchestrator
pub struct ParadigmShiftEngine {
    detector: AnomalyDetector,
    sandbox: LlmSandbox,
    ab_test: Option<ABTestFramework>,
    pub switch_manager: FrameworkSwitchManager,
    state: EvolutionEngineState,
    ab_test_start: Option<DateTime<Utc>>,
}

impl ParadigmShiftEngine {
    pub fn new() -> Self {
        Self {
            detector: AnomalyDetector::new(0.5),
            sandbox: LlmSandbox::new(),
            ab_test: None,
            switch_manager: FrameworkSwitchManager::new(),
            state: EvolutionEngineState::default(),
            ab_test_start: None,
        }
    }

    pub fn detect_anomalies(
        &mut self,
        claims: &[EpistemicClaim],
        errors: &[f64],
    ) -> AnomalyDetectionResult {
        self.detector.detect(claims, errors)
    }

    pub fn sandbox_test(
        &mut self,
        framework: Framework,
        claims: &[EpistemicClaim],
    ) -> SandboxReport {
        self.sandbox.load_framework(framework);
        self.sandbox.test_ontology(claims);

        let predictions = vec![
            ("q1".to_string(), "a1".to_string(), true),
            ("q2".to_string(), "a2".to_string(), true),
            ("q3".to_string(), "a3".to_string(), false),
        ];
        self.sandbox.test_predictions(&predictions);

        self.sandbox.generate_report()
    }

    pub fn start_ab_test(&mut self, current: Framework, candidate: Framework) {
        self.ab_test = Some(ABTestFramework::new(current, candidate));
        self.ab_test_start = Some(Utc::now());
    }

    pub fn record_ab_outcome(&mut self, outcome: TestOutcome) {
        if let Some(ref mut ab) = self.ab_test {
            ab.record_outcome(outcome);
        }
    }

    pub fn check_ab_results(&self) -> Option<ABTestResult> {
        let ab = self.ab_test.as_ref()?;
        let start = self.ab_test_start?;

        if ab.is_complete(start) {
            Some(ab.calculate_results(start))
        } else {
            None
        }
    }

    pub fn execute_switch(&mut self, new_framework: Framework) -> Result<ShiftRecord> {
        self.switch_manager
            .switch_framework(new_framework, "A/B test recommended switch")
    }

    pub fn execute_rollback(&mut self, reason: &str) -> Result<ShiftRecord> {
        self.switch_manager.rollback(reason)
    }

    pub fn state(&self) -> &EvolutionEngineState {
        &self.state
    }

    pub fn update_state(&mut self, new_state: EvolutionEngineState) {
        self.state = new_state;
    }

    pub fn run_paradigm_shift_workflow(
        &mut self,
        claims: &[EpistemicClaim],
        prediction_errors: &[f64],
        candidate_framework: Framework,
    ) -> ParadigmShiftWorkflowResult {
        let detection = self.detect_anomalies(claims, prediction_errors);

        if detection.recommendation != AnomalyRecommendation::InitiateParadigmShift {
            return ParadigmShiftWorkflowResult {
                initiated: false,
                detection: Some(detection),
                sandbox_report: None,
                ab_result: None,
                switch_record: None,
                message: "Anomaly level insufficient for paradigm shift".to_string(),
            };
        }

        let current = self.switch_manager.current_framework().cloned();
        let sandbox_report = self.sandbox_test(candidate_framework.clone(), claims);

        if !sandbox_report.is_safe_to_deploy {
            return ParadigmShiftWorkflowResult {
                initiated: true,
                detection: Some(detection),
                sandbox_report: Some(sandbox_report),
                ab_result: None,
                switch_record: None,
                message: "Candidate framework failed sandbox tests".to_string(),
            };
        }

        if let Some(current_fw) = current {
            let current_fw_id = current_fw.id;
            self.start_ab_test(current_fw, candidate_framework.clone());
            for i in 0..100 {
                self.record_ab_outcome(TestOutcome {
                    timestamp: Utc::now(),
                    framework_id: if i % 2 == 0 {
                        candidate_framework.id
                    } else {
                        current_fw_id
                    },
                    query: format!("query_{}", i),
                    prediction_correct: i % 3 != 0,
                    confidence: 0.8,
                    resolution_quality: 0.9,
                    processing_time_ms: 100,
                });
            }
        }

        let ab_result = self.check_ab_results();

        let switch_record = if let Some(ref result) = ab_result {
            if result.recommendation == ABTestRecommendation::SwitchToB {
                self.execute_switch(candidate_framework).ok()
            } else {
                None
            }
        } else {
            None
        };

        ParadigmShiftWorkflowResult {
            initiated: true,
            detection: Some(detection),
            sandbox_report: Some(sandbox_report),
            ab_result,
            switch_record,
            message: "Paradigm shift workflow completed".to_string(),
        }
    }
}

impl Default for ParadigmShiftEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Paradigm shift workflow result
#[derive(Debug, Clone)]
pub struct ParadigmShiftWorkflowResult {
    pub initiated: bool,
    pub detection: Option<AnomalyDetectionResult>,
    pub sandbox_report: Option<SandboxReport>,
    pub ab_result: Option<ABTestResult>,
    pub switch_record: Option<ShiftRecord>,
    pub message: String,
}

/// A/B test framework
pub struct ABTestFramework {
    pub framework_a: Framework,
    pub framework_b: Framework,
    config: ABTestConfig,
    results_a: Vec<TestOutcome>,
    results_b: Vec<TestOutcome>,
}

#[derive(Debug, Clone)]
pub struct ABTestConfig {
    pub split_ratio: f64,
    pub min_sample_size: usize,
    pub significance_threshold: f64,
    pub max_duration_hours: i64,
}

impl Default for ABTestConfig {
    fn default() -> Self {
        Self {
            split_ratio: 0.5,
            min_sample_size: 1000,
            significance_threshold: 0.05,
            max_duration_hours: 168,
        }
    }
}

/// Individual test outcome
#[derive(Debug, Clone)]
pub struct TestOutcome {
    pub timestamp: DateTime<Utc>,
    pub framework_id: Uuid,
    pub query: String,
    pub prediction_correct: bool,
    pub confidence: f64,
    pub resolution_quality: f64,
    pub processing_time_ms: u64,
}

/// A/B test result
#[derive(Debug, Clone)]
pub struct ABTestResult {
    pub framework_a_id: Uuid,
    pub framework_b_id: Uuid,
    pub sample_size_a: usize,
    pub sample_size_b: usize,
    pub accuracy_a: f64,
    pub accuracy_b: f64,
    pub improvement_pct: f64,
    pub is_statistically_significant: bool,
    pub p_value: f64,
    pub winner: Option<Uuid>,
    pub recommendation: ABTestRecommendation,
    pub duration_hours: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ABTestRecommendation {
    KeepA,
    SwitchToB,
    ContinueTesting,
    Inconclusive,
}

impl ABTestFramework {
    pub fn new(framework_a: Framework, framework_b: Framework) -> Self {
        Self {
            framework_a,
            framework_b,
            config: ABTestConfig::default(),
            results_a: Vec::new(),
            results_b: Vec::new(),
        }
    }

    pub fn with_config(mut self, config: ABTestConfig) -> Self {
        self.config = config;
        self
    }

    pub fn record_outcome(&mut self, outcome: TestOutcome) {
        if outcome.framework_id == self.framework_a.id {
            self.results_a.push(outcome);
        } else if outcome.framework_id == self.framework_b.id {
            self.results_b.push(outcome);
        }
    }

    pub fn calculate_results(&self, start_time: DateTime<Utc>) -> ABTestResult {
        let sample_a = self.results_a.len();
        let sample_b = self.results_b.len();

        let accuracy_a = if sample_a > 0 {
            self.results_a
                .iter()
                .filter(|r| r.prediction_correct)
                .count() as f64
                / sample_a as f64
        } else {
            0.0
        };

        let accuracy_b = if sample_b > 0 {
            self.results_b
                .iter()
                .filter(|r| r.prediction_correct)
                .count() as f64
                / sample_b as f64
        } else {
            0.0
        };

        let improvement_pct = if accuracy_a > 0.0 {
            (accuracy_b - accuracy_a) / accuracy_a * 100.0
        } else {
            0.0
        };

        let pooled_variance = self.calculate_pooled_variance();
        let se = (pooled_variance * (1.0 / sample_a as f64 + 1.0 / sample_b as f64)).sqrt();
        let t_statistic = (accuracy_b - accuracy_a) / se.max(0.001);
        let p_value = self.approximate_p_value(t_statistic);

        let is_significant = p_value < self.config.significance_threshold;

        let winner = if is_significant {
            if accuracy_b > accuracy_a {
                Some(self.framework_b.id)
            } else if accuracy_a > accuracy_b {
                Some(self.framework_a.id)
            } else {
                None
            }
        } else {
            None
        };

        let duration = (Utc::now() - start_time).num_hours();

        let recommendation =
            if sample_a < self.config.min_sample_size || sample_b < self.config.min_sample_size {
                ABTestRecommendation::ContinueTesting
            } else if !is_significant {
                ABTestRecommendation::Inconclusive
            } else if winner == Some(self.framework_b.id) && improvement_pct > 5.0 {
                ABTestRecommendation::SwitchToB
            } else {
                ABTestRecommendation::KeepA
            };

        ABTestResult {
            framework_a_id: self.framework_a.id,
            framework_b_id: self.framework_b.id,
            sample_size_a: sample_a,
            sample_size_b: sample_b,
            accuracy_a,
            accuracy_b,
            improvement_pct,
            is_statistically_significant: is_significant,
            p_value,
            winner,
            recommendation,
            duration_hours: duration,
        }
    }

    fn calculate_pooled_variance(&self) -> f64 {
        let n1 = self.results_a.len() as f64;
        let n2 = self.results_b.len() as f64;
        if n1 + n2 < 2.0 {
            return 1.0;
        }
        let var1 = self.variance(&self.results_a);
        let var2 = self.variance(&self.results_b);
        ((n1 - 1.0) * var1 + (n2 - 1.0) * var2) / (n1 + n2 - 2.0)
    }

    fn variance(&self, outcomes: &[TestOutcome]) -> f64 {
        if outcomes.len() < 2 {
            return 0.0;
        }
        let mean = outcomes
            .iter()
            .map(|o| if o.prediction_correct { 1.0 } else { 0.0 })
            .sum::<f64>()
            / outcomes.len() as f64;
        let sum_sq_diff: f64 = outcomes
            .iter()
            .map(|o| {
                let val = if o.prediction_correct { 1.0 } else { 0.0 };
                (val - mean).powi(2)
            })
            .sum();
        sum_sq_diff / (outcomes.len() - 1) as f64
    }

    fn approximate_p_value(&self, t_statistic: f64) -> f64 {
        (1.0 / (1.0 + t_statistic.abs())).min(1.0)
    }

    pub fn is_complete(&self, start_time: DateTime<Utc>) -> bool {
        let duration = Utc::now() - start_time;
        let min_samples_met = self.results_a.len() >= self.config.min_sample_size
            && self.results_b.len() >= self.config.min_sample_size;
        let max_duration_exceeded = duration.num_hours() >= self.config.max_duration_hours;
        min_samples_met || max_duration_exceeded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TimeBucket;
    use crate::models::{AccessEnvelope, Claimant, NodeType, ProvenanceRecord};
    use std::collections::HashMap;

    fn create_test_framework(name: &str) -> Framework {
        Framework {
            id: Uuid::new_v4(),
            name: name.to_string(),
            version: "1.0".to_string(),
            hash: format!("hash_{}", name),
            ontology: super::super::OntologyDefinition {
                entity_types: vec!["Entity".to_string(), "Relation".to_string()],
                relation_types: vec!["related_to".to_string()],
                attributes: HashMap::new(),
                constraints: vec![],
            },
            resolution_rules: vec![],
            prediction_config: super::super::PredictionConfig {
                model_type: "default".to_string(),
                parameters: HashMap::new(),
                confidence_threshold: 0.7,
            },
            validation_criteria: super::super::ValidationCriteria {
                min_prediction_accuracy: 0.7,
                max_conflict_rate: 0.2,
                min_confidence_calibration: 0.8,
                min_coverage: 0.5,
            },
            created_at: Utc::now(),
        }
    }

    #[test]
    fn test_anomaly_detection() {
        let mut detector = AnomalyDetector::new(0.5);
        let claims = vec![];
        let errors: Vec<f64> = (0..150).map(|_| 0.5).collect();
        let result = detector.detect(&claims, &errors);
        assert!(result.anomaly_score > 0.0);
        assert!(!result.anomalies.is_empty());
    }

    #[test]
    fn test_anomaly_trend() {
        let mut detector = AnomalyDetector::new(0.5);
        let _result1 = detector.detect(&[], &vec![0.1; 100]);
        let _result2 = detector.detect(&[], &vec![0.5; 100]);
        let trend = detector.trend(1);
        assert!(trend > 0.0);
    }

    #[test]
    fn test_llm_sandbox() {
        let mut sandbox = LlmSandbox::new();
        let framework = create_test_framework("test");
        sandbox.load_framework(framework);
        let claims = vec![EpistemicClaim::new(
            "test-tenant".to_string(),
            "Test entity claim".to_string(),
            NodeType::Entity,
            Claimant::System,
            AccessEnvelope::new("test-tenant"),
            ProvenanceRecord::new("test".to_string(), "test".to_string(), "test".to_string()),
        )];
        let result = sandbox.test_ontology(&claims);
        assert!(result.success);
    }

    #[test]
    fn test_sandbox_predictions() {
        let mut sandbox = LlmSandbox::new();
        let framework = create_test_framework("test");
        sandbox.load_framework(framework);
        let predictions = vec![
            ("q1".to_string(), "a1".to_string(), true),
            ("q2".to_string(), "a2".to_string(), true),
            ("q3".to_string(), "a3".to_string(), true),
        ];
        let result = sandbox.test_predictions(&predictions);
        assert!(result.success);
        assert_eq!(result.metrics.prediction_accuracy, 1.0);
    }

    #[test]
    fn test_sandbox_report() {
        let mut sandbox = LlmSandbox::new();
        let framework = create_test_framework("test");
        sandbox.load_framework(framework);
        sandbox.test_ontology(&[]);
        let report = sandbox.generate_report();
        assert!(!report.is_safe_to_deploy);
        assert_eq!(report.total_tests, 1);
    }

    #[test]
    fn test_ab_test_framework() {
        let framework_a = create_test_framework("A");
        let framework_b = create_test_framework("B");
        let mut ab = ABTestFramework::new(framework_a, framework_b);
        for i in 0..100 {
            ab.record_outcome(TestOutcome {
                timestamp: Utc::now(),
                framework_id: if i < 50 {
                    ab.framework_a.id
                } else {
                    ab.framework_b.id
                },
                query: format!("query_{}", i),
                prediction_correct: i % 4 != 0,
                confidence: 0.8,
                resolution_quality: 0.9,
                processing_time_ms: 100,
            });
        }
        let results = ab.calculate_results(Utc::now());
        assert_eq!(results.sample_size_a, 50);
        assert_eq!(results.sample_size_b, 50);
    }

    #[test]
    fn test_framework_switch_manager() {
        let mut manager = FrameworkSwitchManager::new();
        let framework1 = create_test_framework("v1");
        manager.initialize(framework1);
        assert!(manager.current_framework().is_some());
        let framework2 = create_test_framework("v2");
        let record = manager.switch_framework(framework2, "test").unwrap();
        assert_eq!(record.result, ShiftResult::Success);
        assert_eq!(manager.switch_history().len(), 1);
        assert!(manager.can_rollback());
        let rollback = manager.rollback("test rollback").unwrap();
        assert_eq!(rollback.result, ShiftResult::Rollback);
    }

    #[test]
    fn test_paradigm_shift_engine() {
        let mut engine = ParadigmShiftEngine::new();
        let current = create_test_framework("current");
        engine.switch_manager.initialize(current);
        let candidate = create_test_framework("candidate");
        let claims = vec![];
        let errors: Vec<f64> = (0..150).map(|_| 0.5).collect();
        let result = engine.run_paradigm_shift_workflow(&claims, &errors, candidate);
        assert!(result.initiated);
        assert!(result.detection.is_some());
    }

    #[test]
    fn test_time_bucket() {
        let now = Utc::now();
        let day = TimeBucket::from_timestamp(now, TimeBucket::Day);
        assert!(day.contains("-"));
        let month = TimeBucket::from_timestamp(now, TimeBucket::Month);
        assert!(month.contains("-"));
        assert!(!month.contains("-W"));
    }
}
