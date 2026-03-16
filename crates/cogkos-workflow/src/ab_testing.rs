use crate::{NodeResult, Result, WorkflowError};
use dashmap::DashMap;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};
use uuid::Uuid;

/// A/B Test variant definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestVariant {
    pub id: String,
    pub name: String,
    pub weight: f64, // 0.0 - 1.0, must sum to 1.0 across all variants
    pub config: Value,
}

impl TestVariant {
    pub fn new(id: &str, name: &str, weight: f64, config: Value) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            weight,
            config,
        }
    }
}

/// Test result tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub variant_id: String,
    pub execution_id: String,
    pub success: bool,
    pub metrics: HashMap<String, f64>,
    pub duration_ms: u64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub metadata: Option<Value>,
}

/// Test statistics for a variant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantStats {
    pub variant_id: String,
    pub variant_name: String,
    pub total_runs: u64,
    pub successful_runs: u64,
    pub failed_runs: u64,
    pub avg_duration_ms: f64,
    pub metrics: HashMap<String, MetricStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricStats {
    pub sum: f64,
    pub count: u64,
    pub mean: f64,
    pub min: f64,
    pub max: f64,
    pub variance: f64,
}

/// A/B Test definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbTest {
    pub id: String,
    pub name: String,
    pub variants: Vec<TestVariant>,
    pub status: TestStatus,
    pub start_date: Option<chrono::DateTime<chrono::Utc>>,
    pub end_date: Option<chrono::DateTime<chrono::Utc>>,
    pub traffic_allocation: f64, // 0.0 - 1.0, percentage of traffic to test
    pub success_criteria: Option<SuccessCriteria>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestStatus {
    Draft,
    Running,
    Paused,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SuccessCriteria {
    MetricThreshold {
        metric: String,
        min_value: f64,
    },
    MetricComparison {
        metric: String,
        improvement_pct: f64,
    },
    StatisticalSignificance {
        p_value: f64,
    },
}

/// Assignment of a variant to an execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantAssignment {
    pub test_id: String,
    pub variant_id: String,
    pub execution_id: String,
    pub assigned_at: chrono::DateTime<chrono::Utc>,
}

/// A/B Testing Framework
pub struct AbTestFramework {
    tests: Arc<DashMap<String, AbTest>>,
    results: Arc<DashMap<String, Vec<TestResult>>>,
    assignments: Arc<DashMap<String, VariantAssignment>>, // key: execution_id
    stats: Arc<DashMap<String, HashMap<String, VariantStats>>>, // key: test_id
}

impl AbTestFramework {
    pub fn new() -> Self {
        Self {
            tests: Arc::new(DashMap::new()),
            results: Arc::new(DashMap::new()),
            assignments: Arc::new(DashMap::new()),
            stats: Arc::new(DashMap::new()),
        }
    }

    /// Create a new A/B test
    pub fn create_test(&self, name: &str, variants: Vec<TestVariant>) -> Result<String> {
        // Validate weights sum to 1.0
        let total_weight: f64 = variants.iter().map(|v| v.weight).sum();
        if (total_weight - 1.0).abs() > 0.001 {
            return Err(WorkflowError::AbTestError(format!(
                "Variant weights must sum to 1.0, got {}",
                total_weight
            )));
        }

        let test_id = Uuid::new_v4().to_string();
        let test = AbTest {
            id: test_id.clone(),
            name: name.to_string(),
            variants,
            status: TestStatus::Draft,
            start_date: None,
            end_date: None,
            traffic_allocation: 1.0,
            success_criteria: None,
            created_at: chrono::Utc::now(),
        };

        self.tests.insert(test_id.clone(), test);
        self.results.insert(test_id.clone(), Vec::new());
        self.stats.insert(test_id.clone(), HashMap::new());

        info!("Created A/B test: {} ({})", name, test_id);
        Ok(test_id)
    }

    /// Start a test
    pub fn start_test(&self, test_id: &str) -> Result<()> {
        let mut test = self
            .tests
            .get_mut(test_id)
            .ok_or_else(|| WorkflowError::AbTestError("Test not found".to_string()))?;

        if test.status != TestStatus::Draft && test.status != TestStatus::Paused {
            return Err(WorkflowError::AbTestError(format!(
                "Cannot start test in {:?} status",
                test.status
            )));
        }

        test.status = TestStatus::Running;
        test.start_date = Some(chrono::Utc::now());

        info!("Started A/B test: {}", test_id);
        Ok(())
    }

    /// Pause a test
    pub fn pause_test(&self, test_id: &str) -> Result<()> {
        let mut test = self
            .tests
            .get_mut(test_id)
            .ok_or_else(|| WorkflowError::AbTestError("Test not found".to_string()))?;

        if test.status != TestStatus::Running {
            return Err(WorkflowError::AbTestError(
                "Test is not running".to_string(),
            ));
        }

        test.status = TestStatus::Paused;
        info!("Paused A/B test: {}", test_id);
        Ok(())
    }

    /// Stop a test
    pub fn stop_test(&self, test_id: &str) -> Result<()> {
        let mut test = self
            .tests
            .get_mut(test_id)
            .ok_or_else(|| WorkflowError::AbTestError("Test not found".to_string()))?;

        test.status = TestStatus::Completed;
        test.end_date = Some(chrono::Utc::now());
        info!("Stopped A/B test: {}", test_id);
        Ok(())
    }

    /// Assign a variant to an execution
    pub fn assign_variant(&self, test_id: &str, execution_id: &str) -> Result<Option<String>> {
        let test = self
            .tests
            .get(test_id)
            .ok_or_else(|| WorkflowError::AbTestError("Test not found".to_string()))?;

        // Check if test is running
        if test.status != TestStatus::Running {
            return Ok(None);
        }

        // Check traffic allocation
        let mut rng = rand::rng();
        if rng.random::<f64>() > test.traffic_allocation {
            return Ok(None); // Not allocated to test
        }

        // Weighted random selection
        let random_val: f64 = rng.random();
        let mut cumulative = 0.0;
        let mut selected_variant = None;

        for variant in &test.variants {
            cumulative += variant.weight;
            if random_val <= cumulative {
                selected_variant = Some(variant.id.clone());
                break;
            }
        }

        let variant_id = selected_variant
            .ok_or_else(|| WorkflowError::AbTestError("Failed to select variant".to_string()))?;

        // Store assignment
        let assignment = VariantAssignment {
            test_id: test_id.to_string(),
            variant_id: variant_id.clone(),
            execution_id: execution_id.to_string(),
            assigned_at: chrono::Utc::now(),
        };

        self.assignments
            .insert(execution_id.to_string(), assignment);

        debug!(
            "Assigned variant {} to execution {}",
            variant_id, execution_id
        );

        Ok(Some(variant_id))
    }

    /// Record a test result
    pub fn record_result(&self, execution_id: &str, result: TestResult) -> Result<()> {
        let assignment = self
            .assignments
            .get(execution_id)
            .ok_or_else(|| WorkflowError::AbTestError("No variant assignment found".to_string()))?;

        let test_id = assignment.test_id.clone();

        // Store result
        if let Some(mut results) = self.results.get_mut(&test_id) {
            results.push(result.clone());
        }

        // Update stats
        self.update_stats(&test_id, &result);

        debug!(
            "Recorded result for execution {}: success={}",
            execution_id, result.success
        );
        Ok(())
    }

    fn update_stats(&self, test_id: &str, result: &TestResult) {
        let mut stats = self.stats.entry(test_id.to_string()).or_default();

        let variant_stats =
            stats
                .entry(result.variant_id.clone())
                .or_insert_with(|| VariantStats {
                    variant_id: result.variant_id.clone(),
                    variant_name: result.variant_id.clone(), // Could lookup from test
                    total_runs: 0,
                    successful_runs: 0,
                    failed_runs: 0,
                    avg_duration_ms: 0.0,
                    metrics: HashMap::new(),
                });

        // Update basic stats
        variant_stats.total_runs += 1;
        if result.success {
            variant_stats.successful_runs += 1;
        } else {
            variant_stats.failed_runs += 1;
        }

        // Update duration using running average
        variant_stats.avg_duration_ms = (variant_stats.avg_duration_ms
            * (variant_stats.total_runs - 1) as f64
            + result.duration_ms as f64)
            / variant_stats.total_runs as f64;

        // Update metrics
        for (metric_name, value) in &result.metrics {
            let metric_stats = variant_stats
                .metrics
                .entry(metric_name.clone())
                .or_insert_with(|| MetricStats {
                    sum: 0.0,
                    count: 0,
                    mean: 0.0,
                    min: *value,
                    max: *value,
                    variance: 0.0,
                });

            metric_stats.sum += value;
            metric_stats.count += 1;
            metric_stats.min = metric_stats.min.min(*value);
            metric_stats.max = metric_stats.max.max(*value);

            // Update mean
            let delta = value - metric_stats.mean;
            metric_stats.mean += delta / metric_stats.count as f64;

            // Update variance (Welford's algorithm)
            if metric_stats.count > 1 {
                let delta2 = value - metric_stats.mean;
                metric_stats.variance = ((metric_stats.count - 2) as f64 * metric_stats.variance
                    + delta * delta2)
                    / (metric_stats.count - 1) as f64;
            }
        }
    }

    /// Get test statistics
    pub fn get_stats(&self, test_id: &str) -> Option<HashMap<String, VariantStats>> {
        self.stats.get(test_id).map(|s| s.clone())
    }

    /// Get winning variant based on a metric
    pub fn get_winning_variant(&self, test_id: &str, metric: &str) -> Result<Option<String>> {
        let stats = self
            .stats
            .get(test_id)
            .ok_or_else(|| WorkflowError::AbTestError("Test not found".to_string()))?;

        let mut best_variant = None;
        let mut best_value = f64::NEG_INFINITY;

        for (variant_id, variant_stats) in stats.iter() {
            if let Some(metric_stats) = variant_stats.metrics.get(metric)
                && metric_stats.mean > best_value
            {
                best_value = metric_stats.mean;
                best_variant = Some(variant_id.clone());
            }
        }

        Ok(best_variant)
    }

    /// Get test results
    pub fn get_results(&self, test_id: &str) -> Option<Vec<TestResult>> {
        self.results.get(test_id).map(|r| r.clone())
    }

    /// Check if success criteria is met
    pub fn check_success_criteria(&self, test_id: &str) -> Result<bool> {
        let test = self
            .tests
            .get(test_id)
            .ok_or_else(|| WorkflowError::AbTestError("Test not found".to_string()))?;

        let criteria = match &test.success_criteria {
            Some(c) => c,
            None => return Ok(false),
        };

        let stats = self
            .stats
            .get(test_id)
            .ok_or_else(|| WorkflowError::AbTestError("No stats available".to_string()))?;

        match criteria {
            SuccessCriteria::MetricThreshold { metric, min_value } => {
                for (_, variant_stats) in stats.iter() {
                    if let Some(metric_stats) = variant_stats.metrics.get(metric)
                        && metric_stats.mean >= *min_value
                    {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            SuccessCriteria::MetricComparison {
                metric,
                improvement_pct,
            } => {
                // Compare best variant against control (first variant)
                let control = test
                    .variants
                    .first()
                    .ok_or_else(|| WorkflowError::AbTestError("No control variant".to_string()))?;

                let control_stats = stats.get(&control.id);
                let control_value = control_stats
                    .and_then(|s| s.metrics.get(metric))
                    .map(|m| m.mean)
                    .unwrap_or(0.0);

                if control_value == 0.0 {
                    return Ok(false);
                }

                for (variant_id, variant_stats) in stats.iter() {
                    if variant_id == &control.id {
                        continue;
                    }

                    if let Some(metric_stats) = variant_stats.metrics.get(metric) {
                        let improvement =
                            (metric_stats.mean - control_value) / control_value * 100.0;
                        if improvement >= *improvement_pct {
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            }
            SuccessCriteria::StatisticalSignificance { p_value } => {
                // Implement statistical significance test using Z-test for proportions
                // Compare each variant against control (first variant)
                let control = test
                    .variants
                    .first()
                    .ok_or_else(|| WorkflowError::AbTestError("No control variant".to_string()))?;

                let control_stats = stats.get(&control.id);
                let control_total = control_stats.map(|s| s.total_runs).unwrap_or(0);
                let control_success = control_stats.map(|s| s.successful_runs).unwrap_or(0);

                if control_total == 0 {
                    debug!("No control data available yet");
                    return Ok(false);
                }

                for (variant_id, variant_stats) in stats.iter() {
                    if variant_id == &control.id {
                        continue;
                    }

                    let variant_total = variant_stats.total_runs;
                    let variant_success = variant_stats.successful_runs;

                    if variant_total == 0 {
                        continue;
                    }

                    // Calculate Z-score for two proportions
                    let p1 = control_success as f64 / control_total as f64;
                    let p2 = variant_success as f64 / variant_total as f64;
                    let p_pooled = (control_success + variant_success) as f64
                        / (control_total + variant_total) as f64;

                    if p_pooled == 0.0 || p_pooled == 1.0 {
                        continue;
                    }

                    let se = (p_pooled
                        * (1.0 - p_pooled)
                        * (1.0 / control_total as f64 + 1.0 / variant_total as f64))
                        .sqrt();
                    if se == 0.0 {
                        continue;
                    }

                    let z_score = (p2 - p1).abs() / se;

                    // Calculate approximate p-value using normal distribution
                    let p_calculated = 2.0 * (1.0 - Self::normal_cdf(z_score));

                    debug!(
                        "Z-test: control={}/{}, variant={}/{}, p={:.4}",
                        control_success,
                        control_total,
                        variant_success,
                        variant_total,
                        p_calculated
                    );

                    if p_calculated <= *p_value {
                        info!(
                            "Statistical significance achieved: variant {} vs control (p={:.4} <= {:.4})",
                            variant_id, p_calculated, p_value
                        );
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }

    /// Calculate the cumulative distribution function of standard normal distribution
    fn normal_cdf(x: f64) -> f64 {
        // Approximation using error function
        let a1 = 0.254829592;
        let a2 = -0.284496736;
        let a3 = 1.421413741;
        let a4 = -1.453152027;
        let a5 = 1.061405429;
        let p = 0.3275911;

        let sign = if x < 0.0 { -1.0 } else { 1.0 };
        let x = x.abs() / std::f64::consts::SQRT_2;

        let t = 1.0 / (1.0 + p * x);
        let y = 1.0 - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-x * x).exp();

        0.5 * (1.0 + sign * y)
    }

    /// Get all active tests
    pub fn get_active_tests(&self) -> Vec<String> {
        self.tests
            .iter()
            .filter(|t| t.status == TestStatus::Running)
            .map(|t| t.id.clone())
            .collect()
    }

    /// Set success criteria
    pub fn set_success_criteria(&self, test_id: &str, criteria: SuccessCriteria) -> Result<()> {
        let mut test = self
            .tests
            .get_mut(test_id)
            .ok_or_else(|| WorkflowError::AbTestError("Test not found".to_string()))?;

        test.success_criteria = Some(criteria);
        Ok(())
    }

    /// Set traffic allocation
    pub fn set_traffic_allocation(&self, test_id: &str, allocation: f64) -> Result<()> {
        if !(0.0..=1.0).contains(&allocation) {
            return Err(WorkflowError::AbTestError(
                "Traffic allocation must be between 0.0 and 1.0".to_string(),
            ));
        }

        let mut test = self
            .tests
            .get_mut(test_id)
            .ok_or_else(|| WorkflowError::AbTestError("Test not found".to_string()))?;

        test.traffic_allocation = allocation;
        Ok(())
    }
}

impl Default for AbTestFramework {
    fn default() -> Self {
        Self::new()
    }
}

/// A/B Test runner for workflow integration
pub struct AbTestRunner {
    framework: Arc<AbTestFramework>,
}

impl AbTestRunner {
    pub fn new(framework: Arc<AbTestFramework>) -> Self {
        Self { framework }
    }

    /// Run a function with A/B testing
    pub async fn run<F, Fut>(
        &self,
        test_id: &str,
        execution_id: &str,
        run_fn: F,
    ) -> Result<(String, NodeResult)>
    where
        F: FnOnce(Value) -> Fut,
        Fut: std::future::Future<Output = Result<NodeResult>>,
    {
        // Assign variant
        let variant_id = self
            .framework
            .assign_variant(test_id, execution_id)?
            .ok_or_else(|| WorkflowError::AbTestError("Could not assign variant".to_string()))?;

        // Get variant config
        let test = self
            .framework
            .tests
            .get(test_id)
            .ok_or_else(|| WorkflowError::AbTestError("Test not found".to_string()))?;
        let config = test
            .variants
            .iter()
            .find(|v| v.id == variant_id)
            .map(|v| v.config.clone())
            .unwrap_or(Value::Null);

        drop(test);

        // Execute
        let start = std::time::Instant::now();
        let result = run_fn(config).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        // Record result
        match &result {
            Ok(node_result) => {
                let test_result = TestResult {
                    variant_id: variant_id.clone(),
                    execution_id: execution_id.to_string(),
                    success: node_result.success,
                    metrics: extract_metrics(&node_result.output),
                    duration_ms,
                    timestamp: chrono::Utc::now(),
                    metadata: Some(node_result.output.clone()),
                };
                let _ = self.framework.record_result(execution_id, test_result);
            }
            Err(_) => {
                let test_result = TestResult {
                    variant_id: variant_id.clone(),
                    execution_id: execution_id.to_string(),
                    success: false,
                    metrics: HashMap::new(),
                    duration_ms,
                    timestamp: chrono::Utc::now(),
                    metadata: None,
                };
                let _ = self.framework.record_result(execution_id, test_result);
            }
        }

        result.map(|r| (variant_id, r))
    }
}

fn extract_metrics(output: &Value) -> HashMap<String, f64> {
    let mut metrics = HashMap::new();

    if let Some(obj) = output.as_object() {
        for (key, value) in obj {
            if let Some(num) = value.as_f64() {
                metrics.insert(key.clone(), num);
            } else if let Some(num) = value.as_i64() {
                metrics.insert(key.clone(), num as f64);
            }
        }
    }

    metrics
}

/// Paradigm shift A/B test for evolutionary engine
///
/// Used during paradigm shift mode to test new frameworks against old
pub struct ParadigmShiftTest {
    framework: Arc<AbTestFramework>,
    old_framework_id: String,
    new_framework_id: String,
}

impl ParadigmShiftTest {
    pub fn new(
        framework: Arc<AbTestFramework>,
        old_framework_id: String,
        new_framework_id: String,
    ) -> Self {
        Self {
            framework,
            old_framework_id,
            new_framework_id,
        }
    }

    pub fn create_test(&self, test_name: &str) -> Result<String> {
        let variants = vec![
            TestVariant::new(
                &self.old_framework_id,
                "control",
                0.5,
                serde_json::json!({ "framework_id": self.old_framework_id }),
            ),
            TestVariant::new(
                &self.new_framework_id,
                "treatment",
                0.5,
                serde_json::json!({ "framework_id": self.new_framework_id }),
            ),
        ];

        let test_id = self.framework.create_test(test_name, variants)?;

        // Set success criteria: new framework must be 10% better
        self.framework.set_success_criteria(
            &test_id,
            SuccessCriteria::MetricComparison {
                metric: "prediction_accuracy".to_string(),
                improvement_pct: 10.0,
            },
        )?;

        Ok(test_id)
    }

    pub fn evaluate_shift(&self, test_id: &str) -> Result<ShiftDecision> {
        let winning = self
            .framework
            .get_winning_variant(test_id, "prediction_accuracy")?;
        let criteria_met = self.framework.check_success_criteria(test_id)?;

        match winning {
            Some(variant_id) if variant_id == self.new_framework_id && criteria_met => {
                Ok(ShiftDecision::Switch)
            }
            _ => Ok(ShiftDecision::Rollback),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftDecision {
    Switch,
    Rollback,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_variants(weights: &[f64]) -> Vec<TestVariant> {
        weights
            .iter()
            .enumerate()
            .map(|(i, &w)| {
                TestVariant::new(
                    &format!("variant-{}", i),
                    &format!("Variant {}", i),
                    w,
                    json!({"index": i}),
                )
            })
            .collect()
    }

    fn make_two_variants() -> Vec<TestVariant> {
        make_variants(&[0.5, 0.5])
    }

    fn make_result(variant_id: &str, success: bool, metrics: HashMap<String, f64>) -> TestResult {
        TestResult {
            variant_id: variant_id.to_string(),
            execution_id: Uuid::new_v4().to_string(),
            success,
            metrics,
            duration_ms: 100,
            timestamp: chrono::Utc::now(),
            metadata: None,
        }
    }

    // ==================== Group 1: ABTestConfig/types ====================

    #[test]
    fn config_create_test_variant() {
        let v = TestVariant::new("v1", "Control", 0.5, json!({"model": "gpt-4"}));
        assert_eq!(v.id, "v1");
        assert_eq!(v.name, "Control");
        assert!((v.weight - 0.5).abs() < 1e-9);
    }

    #[test]
    fn config_test_variant_config_value() {
        let v = TestVariant::new("v1", "Control", 0.5, json!({"temp": 0.7}));
        assert_eq!(v.config["temp"], 0.7);
    }

    #[test]
    fn config_test_status_values() {
        assert_ne!(TestStatus::Draft, TestStatus::Running);
        assert_ne!(TestStatus::Running, TestStatus::Paused);
        assert_ne!(TestStatus::Completed, TestStatus::Cancelled);
    }

    #[test]
    fn config_success_criteria_threshold() {
        let c = SuccessCriteria::MetricThreshold {
            metric: "accuracy".to_string(),
            min_value: 0.9,
        };
        match c {
            SuccessCriteria::MetricThreshold { metric, min_value } => {
                assert_eq!(metric, "accuracy");
                assert!((min_value - 0.9).abs() < 1e-9);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn config_success_criteria_comparison() {
        let c = SuccessCriteria::MetricComparison {
            metric: "latency".to_string(),
            improvement_pct: 10.0,
        };
        match c {
            SuccessCriteria::MetricComparison {
                metric,
                improvement_pct,
            } => {
                assert_eq!(metric, "latency");
                assert!((improvement_pct - 10.0).abs() < 1e-9);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn config_success_criteria_significance() {
        let c = SuccessCriteria::StatisticalSignificance { p_value: 0.05 };
        match c {
            SuccessCriteria::StatisticalSignificance { p_value } => {
                assert!((p_value - 0.05).abs() < 1e-9);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn config_framework_default() {
        let fw = AbTestFramework::default();
        assert!(fw.get_active_tests().is_empty());
    }

    #[test]
    fn config_create_test_weights_must_sum_to_one() {
        let fw = AbTestFramework::new();
        let variants = make_variants(&[0.3, 0.3]); // sums to 0.6
        let result = fw.create_test("bad-test", variants);
        assert!(result.is_err());
    }

    #[test]
    fn config_variant_assignment_struct() {
        let a = VariantAssignment {
            test_id: "t1".to_string(),
            variant_id: "v1".to_string(),
            execution_id: "e1".to_string(),
            assigned_at: chrono::Utc::now(),
        };
        assert_eq!(a.test_id, "t1");
        assert_eq!(a.variant_id, "v1");
    }

    #[test]
    fn config_test_result_struct() {
        let mut metrics = HashMap::new();
        metrics.insert("accuracy".to_string(), 0.95);
        let r = make_result("v1", true, metrics);
        assert!(r.success);
        assert_eq!(r.metrics["accuracy"], 0.95);
    }

    // ==================== Group 2: Experiment lifecycle ====================

    #[test]
    fn lifecycle_create_experiment() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        assert!(!test_id.is_empty());
    }

    #[test]
    fn lifecycle_initial_status_is_draft() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        let test = fw.tests.get(&test_id).unwrap();
        assert_eq!(test.status, TestStatus::Draft);
    }

    #[test]
    fn lifecycle_start_test() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        fw.start_test(&test_id).unwrap();
        let test = fw.tests.get(&test_id).unwrap();
        assert_eq!(test.status, TestStatus::Running);
        assert!(test.start_date.is_some());
    }

    #[test]
    fn lifecycle_pause_test() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        fw.start_test(&test_id).unwrap();
        fw.pause_test(&test_id).unwrap();
        let test = fw.tests.get(&test_id).unwrap();
        assert_eq!(test.status, TestStatus::Paused);
    }

    #[test]
    fn lifecycle_resume_paused_test() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        fw.start_test(&test_id).unwrap();
        fw.pause_test(&test_id).unwrap();
        fw.start_test(&test_id).unwrap(); // Resume from paused
        let test = fw.tests.get(&test_id).unwrap();
        assert_eq!(test.status, TestStatus::Running);
    }

    #[test]
    fn lifecycle_stop_test() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        fw.start_test(&test_id).unwrap();
        fw.stop_test(&test_id).unwrap();
        let test = fw.tests.get(&test_id).unwrap();
        assert_eq!(test.status, TestStatus::Completed);
        assert!(test.end_date.is_some());
    }

    #[test]
    fn lifecycle_cannot_start_completed_test() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        fw.start_test(&test_id).unwrap();
        fw.stop_test(&test_id).unwrap();
        let result = fw.start_test(&test_id);
        assert!(result.is_err());
    }

    #[test]
    fn lifecycle_cannot_pause_draft_test() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        let result = fw.pause_test(&test_id);
        assert!(result.is_err());
    }

    #[test]
    fn lifecycle_start_nonexistent_test() {
        let fw = AbTestFramework::new();
        let result = fw.start_test("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn lifecycle_active_tests_tracking() {
        let fw = AbTestFramework::new();
        let t1 = fw.create_test("exp-1", make_two_variants()).unwrap();
        let t2 = fw.create_test("exp-2", make_two_variants()).unwrap();
        assert!(fw.get_active_tests().is_empty());

        fw.start_test(&t1).unwrap();
        assert_eq!(fw.get_active_tests().len(), 1);

        fw.start_test(&t2).unwrap();
        assert_eq!(fw.get_active_tests().len(), 2);

        fw.stop_test(&t1).unwrap();
        assert_eq!(fw.get_active_tests().len(), 1);
    }

    // ==================== Group 3: Variant assignment ====================

    #[test]
    fn assign_variant_returns_some_when_running() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        fw.start_test(&test_id).unwrap();
        let assignment = fw.assign_variant(&test_id, "exec-1").unwrap();
        assert!(assignment.is_some());
    }

    #[test]
    fn assign_variant_returns_none_when_draft() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        let assignment = fw.assign_variant(&test_id, "exec-1").unwrap();
        assert!(assignment.is_none());
    }

    #[test]
    fn assign_variant_returns_valid_variant_id() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        fw.start_test(&test_id).unwrap();
        let variant_id = fw.assign_variant(&test_id, "exec-1").unwrap().unwrap();
        assert!(variant_id == "variant-0" || variant_id == "variant-1");
    }

    #[test]
    fn assign_variant_nonexistent_test() {
        let fw = AbTestFramework::new();
        let result = fw.assign_variant("nonexistent", "exec-1");
        assert!(result.is_err());
    }

    #[test]
    fn assign_variant_distribution_roughly_balanced() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        fw.start_test(&test_id).unwrap();

        let mut counts = HashMap::new();
        for i in 0..1000 {
            if let Some(v) = fw
                .assign_variant(&test_id, &format!("exec-{}", i))
                .unwrap()
            {
                *counts.entry(v).or_insert(0u32) += 1;
            }
        }

        // With 50/50 weights and traffic_allocation=1.0, both should get assignments
        assert!(counts.len() == 2);
        let c0 = *counts.get("variant-0").unwrap_or(&0);
        let c1 = *counts.get("variant-1").unwrap_or(&0);
        // Each should be roughly 500 +/- 100
        assert!(c0 > 300 && c0 < 700, "variant-0 count {} out of range", c0);
        assert!(c1 > 300 && c1 < 700, "variant-1 count {} out of range", c1);
    }

    #[test]
    fn assign_variant_custom_weights() {
        let fw = AbTestFramework::new();
        let variants = make_variants(&[0.9, 0.1]);
        let test_id = fw.create_test("exp-weighted", variants).unwrap();
        fw.start_test(&test_id).unwrap();

        let mut counts = HashMap::new();
        for i in 0..1000 {
            if let Some(v) = fw
                .assign_variant(&test_id, &format!("exec-{}", i))
                .unwrap()
            {
                *counts.entry(v).or_insert(0u32) += 1;
            }
        }

        let c0 = *counts.get("variant-0").unwrap_or(&0);
        let c1 = *counts.get("variant-1").unwrap_or(&0);
        // variant-0 should get ~90%, variant-1 ~10%
        assert!(c0 > c1 * 3, "variant-0={} should dominate variant-1={}", c0, c1);
    }

    #[test]
    fn assign_variant_traffic_allocation_zero() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        fw.start_test(&test_id).unwrap();
        fw.set_traffic_allocation(&test_id, 0.0).unwrap();

        // With 0% traffic, all should return None
        let mut none_count = 0;
        for i in 0..100 {
            if fw
                .assign_variant(&test_id, &format!("exec-{}", i))
                .unwrap()
                .is_none()
            {
                none_count += 1;
            }
        }
        assert_eq!(none_count, 100);
    }

    #[test]
    fn assign_variant_traffic_allocation_invalid() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        assert!(fw.set_traffic_allocation(&test_id, 1.5).is_err());
        assert!(fw.set_traffic_allocation(&test_id, -0.1).is_err());
    }

    #[test]
    fn assign_variant_three_variants() {
        let fw = AbTestFramework::new();
        let variants = make_variants(&[0.34, 0.33, 0.33]);
        let test_id = fw.create_test("exp-3way", variants).unwrap();
        fw.start_test(&test_id).unwrap();

        let mut counts = HashMap::new();
        for i in 0..900 {
            if let Some(v) = fw
                .assign_variant(&test_id, &format!("exec-{}", i))
                .unwrap()
            {
                *counts.entry(v).or_insert(0u32) += 1;
            }
        }
        assert_eq!(counts.len(), 3);
    }

    #[test]
    fn assign_variant_stores_assignment() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        fw.start_test(&test_id).unwrap();
        fw.assign_variant(&test_id, "exec-1").unwrap();

        let assignment = fw.assignments.get("exec-1").unwrap();
        assert_eq!(assignment.test_id, test_id);
        assert_eq!(assignment.execution_id, "exec-1");
    }

    // ==================== Group 4: Results and analysis ====================

    #[test]
    fn results_record_and_retrieve() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        fw.start_test(&test_id).unwrap();

        let variant_id = fw.assign_variant(&test_id, "exec-1").unwrap().unwrap();
        let mut metrics = HashMap::new();
        metrics.insert("score".to_string(), 0.95);
        let result = TestResult {
            variant_id: variant_id.clone(),
            execution_id: "exec-1".to_string(),
            success: true,
            metrics,
            duration_ms: 150,
            timestamp: chrono::Utc::now(),
            metadata: None,
        };
        fw.record_result("exec-1", result).unwrap();

        let results = fw.get_results(&test_id).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
    }

    #[test]
    fn results_record_without_assignment_fails() {
        let fw = AbTestFramework::new();
        let result = make_result("v1", true, HashMap::new());
        assert!(fw.record_result("no-assignment", result).is_err());
    }

    #[test]
    fn results_stats_update_on_record() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        fw.start_test(&test_id).unwrap();

        let variant_id = fw.assign_variant(&test_id, "exec-1").unwrap().unwrap();
        let result = TestResult {
            variant_id: variant_id.clone(),
            execution_id: "exec-1".to_string(),
            success: true,
            metrics: HashMap::new(),
            duration_ms: 200,
            timestamp: chrono::Utc::now(),
            metadata: None,
        };
        fw.record_result("exec-1", result).unwrap();

        let stats = fw.get_stats(&test_id).unwrap();
        let vs = stats.get(&variant_id).unwrap();
        assert_eq!(vs.total_runs, 1);
        assert_eq!(vs.successful_runs, 1);
        assert_eq!(vs.failed_runs, 0);
        assert!((vs.avg_duration_ms - 200.0).abs() < 1e-9);
    }

    #[test]
    fn results_stats_failed_runs() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        fw.start_test(&test_id).unwrap();

        let variant_id = fw.assign_variant(&test_id, "exec-1").unwrap().unwrap();
        let result = TestResult {
            variant_id: variant_id.clone(),
            execution_id: "exec-1".to_string(),
            success: false,
            metrics: HashMap::new(),
            duration_ms: 50,
            timestamp: chrono::Utc::now(),
            metadata: None,
        };
        fw.record_result("exec-1", result).unwrap();

        let stats = fw.get_stats(&test_id).unwrap();
        let vs = stats.get(&variant_id).unwrap();
        assert_eq!(vs.failed_runs, 1);
        assert_eq!(vs.successful_runs, 0);
    }

    #[test]
    fn results_metric_stats_calculation() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_variants(&[1.0])).unwrap();
        fw.start_test(&test_id).unwrap();

        let values = [10.0, 20.0, 30.0];
        for (i, &val) in values.iter().enumerate() {
            let exec_id = format!("exec-{}", i);
            fw.assign_variant(&test_id, &exec_id).unwrap();
            let mut metrics = HashMap::new();
            metrics.insert("score".to_string(), val);
            let result = TestResult {
                variant_id: "variant-0".to_string(),
                execution_id: exec_id.clone(),
                success: true,
                metrics,
                duration_ms: 100,
                timestamp: chrono::Utc::now(),
                metadata: None,
            };
            fw.record_result(&exec_id, result).unwrap();
        }

        let stats = fw.get_stats(&test_id).unwrap();
        let vs = stats.get("variant-0").unwrap();
        let ms = vs.metrics.get("score").unwrap();
        assert_eq!(ms.count, 3);
        assert!((ms.sum - 60.0).abs() < 1e-9);
        assert!((ms.mean - 20.0).abs() < 1e-9);
        assert!((ms.min - 10.0).abs() < 1e-9);
        assert!((ms.max - 30.0).abs() < 1e-9);
    }

    #[test]
    fn results_winning_variant() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        fw.start_test(&test_id).unwrap();

        // Record higher score for variant-0
        for i in 0..10 {
            let exec_id = format!("exec-a-{}", i);
            fw.assign_variant(&test_id, &exec_id).unwrap();
            // Force assignment to variant-0
            fw.assignments.insert(
                exec_id.clone(),
                VariantAssignment {
                    test_id: test_id.clone(),
                    variant_id: "variant-0".to_string(),
                    execution_id: exec_id.clone(),
                    assigned_at: chrono::Utc::now(),
                },
            );
            let mut metrics = HashMap::new();
            metrics.insert("score".to_string(), 90.0);
            let result = TestResult {
                variant_id: "variant-0".to_string(),
                execution_id: exec_id.clone(),
                success: true,
                metrics,
                duration_ms: 100,
                timestamp: chrono::Utc::now(),
                metadata: None,
            };
            fw.record_result(&exec_id, result).unwrap();
        }

        // Record lower score for variant-1
        for i in 0..10 {
            let exec_id = format!("exec-b-{}", i);
            fw.assignments.insert(
                exec_id.clone(),
                VariantAssignment {
                    test_id: test_id.clone(),
                    variant_id: "variant-1".to_string(),
                    execution_id: exec_id.clone(),
                    assigned_at: chrono::Utc::now(),
                },
            );
            let mut metrics = HashMap::new();
            metrics.insert("score".to_string(), 50.0);
            let result = TestResult {
                variant_id: "variant-1".to_string(),
                execution_id: exec_id.clone(),
                success: true,
                metrics,
                duration_ms: 100,
                timestamp: chrono::Utc::now(),
                metadata: None,
            };
            fw.record_result(&exec_id, result).unwrap();
        }

        let winner = fw.get_winning_variant(&test_id, "score").unwrap();
        assert_eq!(winner, Some("variant-0".to_string()));
    }

    #[test]
    fn results_winning_variant_no_metric() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        fw.start_test(&test_id).unwrap();

        let winner = fw.get_winning_variant(&test_id, "nonexistent").unwrap();
        assert!(winner.is_none());
    }

    #[test]
    fn results_empty_results() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        let results = fw.get_results(&test_id).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn results_get_stats_empty() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        let stats = fw.get_stats(&test_id).unwrap();
        assert!(stats.is_empty());
    }

    #[test]
    fn results_success_criteria_threshold_met() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_variants(&[1.0])).unwrap();
        fw.start_test(&test_id).unwrap();
        fw.set_success_criteria(
            &test_id,
            SuccessCriteria::MetricThreshold {
                metric: "accuracy".to_string(),
                min_value: 0.8,
            },
        )
        .unwrap();

        let exec_id = "exec-1";
        fw.assign_variant(&test_id, exec_id).unwrap();
        let mut metrics = HashMap::new();
        metrics.insert("accuracy".to_string(), 0.95);
        let result = TestResult {
            variant_id: "variant-0".to_string(),
            execution_id: exec_id.to_string(),
            success: true,
            metrics,
            duration_ms: 100,
            timestamp: chrono::Utc::now(),
            metadata: None,
        };
        fw.record_result(exec_id, result).unwrap();

        assert!(fw.check_success_criteria(&test_id).unwrap());
    }

    #[test]
    fn results_success_criteria_threshold_not_met() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_variants(&[1.0])).unwrap();
        fw.start_test(&test_id).unwrap();
        fw.set_success_criteria(
            &test_id,
            SuccessCriteria::MetricThreshold {
                metric: "accuracy".to_string(),
                min_value: 0.99,
            },
        )
        .unwrap();

        let exec_id = "exec-1";
        fw.assign_variant(&test_id, exec_id).unwrap();
        let mut metrics = HashMap::new();
        metrics.insert("accuracy".to_string(), 0.5);
        let result = TestResult {
            variant_id: "variant-0".to_string(),
            execution_id: exec_id.to_string(),
            success: true,
            metrics,
            duration_ms: 100,
            timestamp: chrono::Utc::now(),
            metadata: None,
        };
        fw.record_result(exec_id, result).unwrap();

        assert!(!fw.check_success_criteria(&test_id).unwrap());
    }

    #[test]
    fn results_no_success_criteria_returns_false() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_two_variants()).unwrap();
        assert!(!fw.check_success_criteria(&test_id).unwrap());
    }

    #[test]
    fn results_multiple_metrics_tracked() {
        let fw = AbTestFramework::new();
        let test_id = fw.create_test("exp-1", make_variants(&[1.0])).unwrap();
        fw.start_test(&test_id).unwrap();

        let exec_id = "exec-1";
        fw.assign_variant(&test_id, exec_id).unwrap();
        let mut metrics = HashMap::new();
        metrics.insert("accuracy".to_string(), 0.9);
        metrics.insert("latency".to_string(), 150.0);
        metrics.insert("throughput".to_string(), 1000.0);
        let result = TestResult {
            variant_id: "variant-0".to_string(),
            execution_id: exec_id.to_string(),
            success: true,
            metrics,
            duration_ms: 100,
            timestamp: chrono::Utc::now(),
            metadata: None,
        };
        fw.record_result(exec_id, result).unwrap();

        let stats = fw.get_stats(&test_id).unwrap();
        let vs = stats.get("variant-0").unwrap();
        assert!(vs.metrics.contains_key("accuracy"));
        assert!(vs.metrics.contains_key("latency"));
        assert!(vs.metrics.contains_key("throughput"));
    }
}
