//! AutoSwitchController implementation

use super::*;
use chrono::Utc;
use std::collections::HashMap;

/// Automated switch controller
pub struct AutoSwitchController {
    config: AutoSwitchConfig,
    status: AutoSwitchStatus,
    /// Current A/B test ID
    current_test_id: Option<String>,
    /// Current rollout phase
    rollout_phase: RolloutPhase,
    /// Last rollout update time
    last_rollout_update: Option<DateTime<Utc>>,
    /// Rollback counter
    rollback_count: u32,
    /// Rollout history
    rollout_history: Vec<RolloutRecord>,
    /// Metrics by test ID
    test_metrics: HashMap<String, TestMetrics>,
}

impl AutoSwitchController {
    pub fn new() -> Self {
        Self {
            config: AutoSwitchConfig::default(),
            status: AutoSwitchStatus::Disabled,
            current_test_id: None,
            rollout_phase: RolloutPhase::None,
            last_rollout_update: None,
            rollback_count: 0,
            rollout_history: Vec::new(),
            test_metrics: HashMap::new(),
        }
    }

    pub fn with_config(mut self, config: AutoSwitchConfig) -> Self {
        self.config = config;
        self
    }

    /// Enable automated switching
    pub fn enable(&mut self) {
        if self.config.enabled {
            self.status = AutoSwitchStatus::Monitoring;
        }
    }

    /// Disable automated switching
    pub fn disable(&mut self) {
        self.status = AutoSwitchStatus::Disabled;
    }

    /// Get current status
    pub fn status(&self) -> AutoSwitchStatus {
        self.status
    }

    /// Start a new A/B test
    pub fn start_test(&mut self, test_id: String) {
        self.current_test_id = Some(test_id.clone());
        self.test_metrics.insert(test_id, TestMetrics::default());
        self.status = AutoSwitchStatus::Testing;
    }

    /// Record a test outcome
    pub fn record_outcome(
        &mut self,
        variant: &str,
        correct: bool,
        latency_ms: f64,
        had_conflict: bool,
    ) {
        let test_id = match &self.current_test_id {
            Some(id) => id,
            None => return,
        };

        let metrics = self.test_metrics.entry(test_id.clone()).or_default();

        match variant {
            "control" | "A" => {
                metrics.control_total += 1;
                if correct {
                    metrics.control_correct += 1;
                }
                metrics.control_avg_latency_ms = (metrics.control_avg_latency_ms
                    * (metrics.control_total - 1) as f64
                    + latency_ms)
                    / metrics.control_total as f64;
                if had_conflict {
                    metrics.control_conflicts += 1;
                }
            }
            "treatment" | "B" => {
                metrics.treatment_total += 1;
                if correct {
                    metrics.treatment_correct += 1;
                }
                metrics.treatment_avg_latency_ms = (metrics.treatment_avg_latency_ms
                    * (metrics.treatment_total - 1) as f64
                    + latency_ms)
                    / metrics.treatment_total as f64;
                if had_conflict {
                    metrics.treatment_conflicts += 1;
                }
            }
            _ => {}
        }
    }

    /// Check if we should initiate a switch based on current metrics
    pub fn should_switch(&self) -> Option<SwitchRecommendation> {
        let test_id = match &self.current_test_id {
            Some(id) => id,
            None => return None,
        };

        let metrics = self.test_metrics.get(test_id)?;

        // Check minimum sample size
        let total = metrics.control_total + metrics.treatment_total;
        if total < self.config.min_sample_size as u64 {
            return None;
        }

        // Check statistical significance
        if !metrics.is_significant(self.config.significance_threshold) {
            return None;
        }

        // Check improvement threshold
        let improvement = metrics.improvement_pct();
        if improvement < self.config.min_improvement_pct {
            return None;
        }

        Some(SwitchRecommendation {
            test_id: test_id.clone(),
            recommended_phase: if improvement > 20.0 {
                RolloutPhase::Partial
            } else {
                RolloutPhase::Initial
            },
            improvement_pct: improvement,
            p_value: metrics.p_value(),
            reason: if improvement > 20.0 {
                "Strong improvement (>20%)".to_string()
            } else {
                "Moderate improvement".to_string()
            },
        })
    }

    /// Initiate gradual rollout
    pub fn initiate_rollout(&mut self, phase: RolloutPhase, reason: RolloutReason) {
        let test_id = match &self.current_test_id {
            Some(id) => id.clone(),
            None => return,
        };

        let metrics = self.test_metrics.get(&test_id).cloned().unwrap_or_default();

        let record = RolloutRecord {
            timestamp: Utc::now(),
            from_phase: self.rollout_phase,
            to_phase: phase,
            reason,
            test_metrics: metrics,
        };

        self.rollout_history.push(record);
        self.rollout_phase = phase;
        self.last_rollout_update = Some(Utc::now());
        self.status = AutoSwitchStatus::GradualRollout;
    }

    /// Check if it's time to advance rollout phase
    pub fn check_rollout_progression(&mut self) -> Option<RolloutPhase> {
        if self.status != AutoSwitchStatus::GradualRollout {
            return None;
        }

        // Check time since last update
        if let Some(last) = self.last_rollout_update {
            let hours_since = (Utc::now() - last).num_hours();
            if hours_since < self.config.rollout_interval_hours {
                return None;
            }
        }

        let test_id = match &self.current_test_id {
            Some(id) => id,
            None => return None,
        };

        let metrics = self.test_metrics.get(test_id)?;

        // Check if performance is still good
        let improvement = metrics.improvement_pct();
        if improvement < self.config.min_improvement_pct {
            // Performance degraded, trigger rollback
            self.status = AutoSwitchStatus::RollingBack;
            return None;
        }

        // Advance to next phase
        let next = self.rollout_phase.next();
        if next != self.rollout_phase {
            Some(next)
        } else {
            None
        }
    }

    /// Check if rollback should be triggered
    pub fn should_rollback(&self) -> Option<RolloutReason> {
        if self.status != AutoSwitchStatus::GradualRollout
            && self.status != AutoSwitchStatus::FullyDeployed
        {
            return None;
        }

        if self.rollback_count >= self.config.max_rollback_attempts {
            return None;
        }

        let test_id = match &self.current_test_id {
            Some(id) => id,
            None => return None,
        };

        let metrics = self.test_metrics.get(test_id)?;

        let improvement = metrics.improvement_pct();
        if improvement < self.config.rollback_threshold_pct {
            return Some(RolloutReason::Failed);
        }

        None
    }

    /// Execute rollback
    pub fn execute_rollback(&mut self, reason: RolloutReason) -> Result<ShiftRecord> {
        if self.rollback_count >= self.config.max_rollback_attempts {
            return Err(AutoSwitchError::RollbackFailed(
                "Max rollback attempts reached".to_string(),
            ));
        }

        self.rollback_count += 1;
        self.status = AutoSwitchStatus::RollingBack;

        let test_id = match &self.current_test_id {
            Some(id) => id.clone(),
            None => {
                return Err(AutoSwitchError::RollbackFailed(
                    "No active test".to_string(),
                ));
            }
        };

        let metrics = self.test_metrics.get(&test_id).cloned().unwrap_or_default();

        let record = RolloutRecord {
            timestamp: Utc::now(),
            from_phase: self.rollout_phase,
            to_phase: RolloutPhase::None,
            reason,
            test_metrics: metrics.clone(),
        };

        self.rollout_history.push(record);

        let improvement = metrics.improvement_pct();

        Ok(ShiftRecord {
            timestamp: Utc::now(),
            result: ShiftResult::Rollback,
            old_framework_hash: format!("test_{}_treatment", test_id),
            new_framework_hash: Some(format!("test_{}_control", test_id)),
            improvement_pct: Some(improvement),
        })
    }

    /// Complete rollout - full deployment achieved
    pub fn complete_rollout(&mut self) {
        self.status = AutoSwitchStatus::FullyDeployed;
        self.initiate_rollout(RolloutPhase::Full, RolloutReason::MetricsGood);
    }

    /// Reset controller for new test
    pub fn reset(&mut self) {
        self.status = AutoSwitchStatus::Monitoring;
        self.current_test_id = None;
        self.rollout_phase = RolloutPhase::None;
        self.last_rollout_update = None;
    }

    /// Get current rollout phase
    pub fn rollout_phase(&self) -> RolloutPhase {
        self.rollout_phase
    }

    /// Get current test metrics
    pub fn get_current_metrics(&self) -> Option<TestMetrics> {
        let test_id = match &self.current_test_id {
            Some(id) => id,
            None => return None,
        };
        self.test_metrics.get(test_id).cloned()
    }

    /// Get rollout history
    pub fn rollout_history(&self) -> &[RolloutRecord] {
        &self.rollout_history
    }

    /// Get switch recommendation
    pub fn get_recommendation(&self) -> Option<SwitchRecommendation> {
        self.should_switch()
    }
}

impl Default for AutoSwitchController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_switch_controller_initialization() {
        let controller = AutoSwitchController::new();
        assert_eq!(controller.status(), AutoSwitchStatus::Disabled);
    }

    #[test]
    fn test_record_outcome() {
        let mut controller = AutoSwitchController::new();
        controller.enable();
        controller.start_test("test-1".to_string());

        // Record some outcomes for control
        for _ in 0..100 {
            controller.record_outcome("control", true, 100.0, false);
        }

        // Record some outcomes for treatment (better)
        for _ in 0..100 {
            controller.record_outcome("treatment", true, 90.0, false);
        }

        let metrics = controller.get_current_metrics().unwrap();
        assert_eq!(metrics.control_total, 100);
        assert_eq!(metrics.treatment_total, 100);
    }

    #[test]
    fn test_should_switch() {
        let mut controller = AutoSwitchController::new();
        controller.enable();
        controller.start_test("test-1".to_string());

        // Control: 40% accuracy (400/1000)
        for _ in 0..600 {
            controller.record_outcome("control", false, 100.0, false);
        }
        for _ in 0..400 {
            controller.record_outcome("control", true, 100.0, false);
        }

        // Treatment: 60% accuracy (600/1000) - 50% improvement over control
        for _ in 0..400 {
            controller.record_outcome("treatment", false, 100.0, false);
        }
        for _ in 0..600 {
            controller.record_outcome("treatment", true, 100.0, false);
        }

        let recommendation = controller.should_switch();
        assert!(recommendation.is_some());

        let rec = recommendation.unwrap();
        assert!(rec.improvement_pct > 40.0);
    }

    #[test]
    fn test_rollout_phases() {
        let mut controller = AutoSwitchController::new();
        controller.enable();
        controller.start_test("test-1".to_string());

        assert_eq!(controller.rollout_phase(), RolloutPhase::None);

        controller.initiate_rollout(RolloutPhase::Initial, RolloutReason::Initial);
        assert_eq!(controller.rollout_phase(), RolloutPhase::Initial);

        let next = controller.rollout_phase().next();
        assert_eq!(next, RolloutPhase::Partial);
    }

    #[test]
    fn test_rollback() {
        let mut controller = AutoSwitchController::new();
        controller.enable();
        controller.start_test("test-1".to_string());
        controller.initiate_rollout(RolloutPhase::Initial, RolloutReason::Initial);

        // Record poor performance for treatment
        for _ in 0..1000 {
            controller.record_outcome("treatment", false, 1000.0, true);
        }
        for _ in 0..1000 {
            controller.record_outcome("control", true, 100.0, false);
        }

        let should_rollback = controller.should_rollback();
        assert!(should_rollback.is_some());
    }

    #[test]
    fn test_test_metrics_improvement() {
        let metrics = TestMetrics {
            control_correct: 400,
            control_total: 1000,
            treatment_correct: 600,
            treatment_total: 1000,
            control_avg_latency_ms: 100.0,
            treatment_avg_latency_ms: 90.0,
            control_conflicts: 100,
            treatment_conflicts: 50,
        };

        assert_eq!(metrics.control_accuracy(), 0.4);
        assert_eq!(metrics.treatment_accuracy(), 0.6);
        assert!((metrics.improvement_pct() - 50.0).abs() < 0.1);
    }
}
