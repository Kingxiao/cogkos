//! Periodic task spawn implementations for the Scheduler.
//!
//! Each task follows the same pattern:
//! - Interval-based tick with cancellation support
//! - Circuit breaker: exponential backoff after consecutive failures
//! - Budget check before execution
//! - Time budget warning

use crate::conflict::detect_conflicts_periodic;
use crate::consolidate::ConsolidationConfig;
use crate::decay::DecayConfig;
use cogkos_core::models::{AnomalySignals, EvolutionMode};
use std::time::Duration;
use tokio::time::interval;
use tracing::{error, info, warn};

use super::TaskType;
use super::core::Scheduler;
use super::tasks::{
    run_confidence_boost, run_consolidation, run_decay, run_health_check, run_insight_extraction,
    run_memory_gc, run_memory_promotion, run_pending_aggregation, run_prediction_validation,
};
use super::tasks_phase3::{
    run_collective_wisdom_check, run_framework_health_monitoring, run_paradigm_shift_check,
};

/// Circuit breaker constants shared by all tasks
pub(crate) const MAX_FAILURES: u32 = 5;
pub(crate) const BACKOFF_MULTIPLIER: u64 = 2;
pub(crate) const MAX_BACKOFF_SECS: u64 = 3600;

impl Scheduler {
    /// Spawn all periodic background tasks
    pub(crate) fn spawn_periodic_tasks(&self) {
        self.spawn_pending_aggregation();
        self.spawn_consolidation();
        self.spawn_decay();
        self.spawn_health_check();
        self.spawn_conflict_detection_periodic();
        self.spawn_confidence_boost();
        self.spawn_memory_gc();
        self.spawn_insight_extraction();
        self.spawn_prediction_validation();
        self.spawn_memory_promotion();
        self.spawn_paradigm_shift_check();
        self.spawn_framework_health();
        self.spawn_collective_wisdom_check();
    }

    fn spawn_pending_aggregation(&self) {
        let s = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(
                s.config.consolidation_event_driven_interval_secs,
            ));
            let mut failures: u32 = 0;
            loop {
                tokio::select! {
                    _ = s.cancel.cancelled() => { info!("PendingAggregation task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if failures >= MAX_FAILURES {
                    let backoff = compute_backoff(
                        s.config.consolidation_event_driven_interval_secs,
                        failures,
                    );
                    warn!(
                        task = "pending_aggregation",
                        failures, "Circuit breaker: backing off"
                    );
                    tokio::time::sleep(backoff).await;
                }
                if !s
                    .check_budget(
                        TaskType::ConsolidationEventDriven,
                        s.config.conflict_batch_size as u64,
                    )
                    .await
                {
                    continue;
                }
                let start_time = std::time::Instant::now();
                let cfg = ConsolidationConfig {
                    batch_size: s.config.conflict_batch_size,
                    ..ConsolidationConfig::default()
                };
                match run_pending_aggregation(&s.stores, &cfg).await {
                    Ok(count) => {
                        failures = 0;
                        s.record_processed(TaskType::ConsolidationEventDriven, count as u64)
                            .await;
                        info!(processed = count, "Processed PendingAggregation claims");
                    }
                    Err(e) => {
                        failures += 1;
                        error!(task = "pending_aggregation", error = %e, failures, "Task failed");
                        s.state.write().await.errors += 1;
                    }
                }
                check_time_budget(
                    start_time,
                    s.config.max_task_duration_ms,
                    "PendingAggregation",
                );
            }
        });
    }

    fn spawn_consolidation(&self) {
        let s = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(s.config.consolidation_interval_secs));
            let mut failures: u32 = 0;
            loop {
                tokio::select! {
                    _ = s.cancel.cancelled() => { info!("Consolidation task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if failures >= MAX_FAILURES {
                    let backoff = compute_backoff(s.config.consolidation_interval_secs, failures);
                    warn!(
                        task = "consolidation",
                        failures, "Circuit breaker: backing off"
                    );
                    tokio::time::sleep(backoff).await;
                }
                if !s
                    .check_budget(TaskType::Consolidation, s.config.conflict_batch_size as u64)
                    .await
                {
                    continue;
                }
                let start_time = std::time::Instant::now();
                let cfg = ConsolidationConfig {
                    batch_size: s.config.conflict_batch_size,
                    ..ConsolidationConfig::default()
                };
                match run_consolidation(&s.stores, &cfg).await {
                    Ok(count) => {
                        failures = 0;
                        s.record_processed(TaskType::Consolidation, count as u64)
                            .await;
                        s.state.write().await.last_consolidation = Some(chrono::Utc::now());
                        // Tick evolution engine with anomaly signals
                        let signals = AnomalySignals {
                            prediction_error_streak: 0, // TODO: wire up from PredictionHistoryStore
                            conflict_density_pct: 0.0,  // TODO: compute from recent conflicts
                            cache_hit_rate_trend: 0.0,  // TODO: compute from cache stats
                        };
                        let mut engine = s.evolution.write().await;
                        engine.tick(signals);
                        if engine.state().mode == EvolutionMode::ParadigmShift {
                            warn!(
                                anomaly_counter = engine.state().anomaly_counter,
                                "Evolution engine triggered ParadigmShift mode"
                            );
                        }
                    }
                    Err(e) => {
                        failures += 1;
                        error!(task = "consolidation", error = %e, failures, "Task failed");
                        s.state.write().await.errors += 1;
                    }
                }
                check_time_budget(start_time, s.config.max_task_duration_ms, "Consolidation");
            }
        });
    }

    fn spawn_decay(&self) {
        let s = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(s.config.decay_interval_secs));
            let mut failures: u32 = 0;
            loop {
                tokio::select! {
                    _ = s.cancel.cancelled() => { info!("Decay task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if failures >= MAX_FAILURES {
                    let backoff = compute_backoff(s.config.decay_interval_secs, failures);
                    warn!(task = "decay", failures, "Circuit breaker: backing off");
                    tokio::time::sleep(backoff).await;
                }
                if !s
                    .check_budget(TaskType::Decay, s.config.conflict_batch_size as u64)
                    .await
                {
                    continue;
                }
                let start_time = std::time::Instant::now();
                let cfg = DecayConfig {
                    batch_size: s.config.conflict_batch_size,
                    ..DecayConfig::default()
                };
                match run_decay(&s.stores, &cfg).await {
                    Ok(count) => {
                        failures = 0;
                        s.record_processed(TaskType::Decay, count as u64).await;
                        s.state.write().await.last_decay = Some(chrono::Utc::now());
                    }
                    Err(e) => {
                        failures += 1;
                        error!(task = "decay", error = %e, failures, "Task failed");
                        s.state.write().await.errors += 1;
                    }
                }
                check_time_budget(start_time, s.config.max_task_duration_ms, "Decay");
            }
        });
    }

    fn spawn_health_check(&self) {
        let s = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(s.config.health_check_interval_secs));
            let mut failures: u32 = 0;
            loop {
                tokio::select! {
                    _ = s.cancel.cancelled() => { info!("Health check task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if failures >= MAX_FAILURES {
                    let backoff = compute_backoff(s.config.health_check_interval_secs, failures);
                    warn!(
                        task = "health_check",
                        failures, "Circuit breaker: backing off"
                    );
                    tokio::time::sleep(backoff).await;
                }
                if !s.check_budget(TaskType::HealthCheck, 10).await {
                    continue;
                }
                match run_health_check(&s.stores).await {
                    Ok(()) => {
                        failures = 0;
                        s.record_processed(TaskType::HealthCheck, 1).await;
                    }
                    Err(e) => {
                        failures += 1;
                        error!(task = "health_check", error = %e, failures, "Task failed");
                        s.state.write().await.errors += 1;
                    }
                }
            }
        });
    }

    fn spawn_conflict_detection_periodic(&self) {
        let s = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(s.config.conflict_interval_secs));
            let mut failures: u32 = 0;
            loop {
                tokio::select! {
                    _ = s.cancel.cancelled() => { info!("Conflict detection task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if failures >= MAX_FAILURES {
                    let backoff = compute_backoff(s.config.conflict_interval_secs, failures);
                    warn!(
                        task = "conflict_detection_periodic",
                        failures, "Circuit breaker: backing off"
                    );
                    tokio::time::sleep(backoff).await;
                }
                if !s
                    .check_budget(
                        TaskType::ConflictDetectionPeriodic,
                        s.config.conflict_batch_size as u64,
                    )
                    .await
                {
                    continue;
                }
                let start_time = std::time::Instant::now();
                match detect_conflicts_periodic(&s.stores, "default", s.config.conflict_batch_size)
                    .await
                {
                    Ok(conflicts) => {
                        failures = 0;
                        s.record_processed(
                            TaskType::ConflictDetectionPeriodic,
                            conflicts.len() as u64,
                        )
                        .await;
                        s.state.write().await.last_conflict_detection = Some(chrono::Utc::now());
                    }
                    Err(e) => {
                        failures += 1;
                        error!(task = "conflict_detection_periodic", error = %e, failures, "Task failed");
                        s.state.write().await.errors += 1;
                    }
                }
                check_time_budget(
                    start_time,
                    s.config.max_task_duration_ms,
                    "Conflict detection",
                );
            }
        });
    }

    fn spawn_confidence_boost(&self) {
        let s = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(s.config.confidence_boost_interval_secs));
            let mut failures: u32 = 0;
            loop {
                tokio::select! {
                    _ = s.cancel.cancelled() => { info!("Confidence boost task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if failures >= MAX_FAILURES {
                    let backoff =
                        compute_backoff(s.config.confidence_boost_interval_secs, failures);
                    warn!(
                        task = "confidence_boost",
                        failures, "Circuit breaker: backing off"
                    );
                    tokio::time::sleep(backoff).await;
                }
                if !s
                    .check_budget(
                        TaskType::ConfidenceBoost,
                        s.config.confidence_boost_batch_size as u64,
                    )
                    .await
                {
                    continue;
                }
                let start_time = std::time::Instant::now();
                match run_confidence_boost(
                    &s.stores,
                    "default",
                    s.config.confidence_boost_batch_size,
                    s.config.confidence_boost_factor,
                )
                .await
                {
                    Ok(count) => {
                        failures = 0;
                        s.record_processed(TaskType::ConfidenceBoost, count as u64)
                            .await;
                        s.state.write().await.last_confidence_boost = Some(chrono::Utc::now());
                    }
                    Err(e) => {
                        failures += 1;
                        error!(task = "confidence_boost", error = %e, failures, "Task failed");
                        s.state.write().await.errors += 1;
                    }
                }
                check_time_budget(
                    start_time,
                    s.config.max_task_duration_ms,
                    "Confidence boost",
                );
            }
        });
    }

    fn spawn_memory_gc(&self) {
        let s = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(s.config.memory_gc_interval_secs));
            let mut failures: u32 = 0;
            loop {
                tokio::select! {
                    _ = s.cancel.cancelled() => { info!("Memory GC task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if failures >= MAX_FAILURES {
                    let backoff = compute_backoff(s.config.memory_gc_interval_secs, failures);
                    warn!(task = "memory_gc", failures, "Circuit breaker: backing off");
                    tokio::time::sleep(backoff).await;
                }
                if !s.check_budget(TaskType::MemoryGc, 100).await {
                    continue;
                }
                match run_memory_gc(&s.stores).await {
                    Ok(count) => {
                        failures = 0;
                        s.record_processed(TaskType::MemoryGc, count as u64).await;
                    }
                    Err(e) => {
                        failures += 1;
                        error!(task = "memory_gc", error = %e, failures, "Task failed");
                        s.state.write().await.errors += 1;
                    }
                }
            }
        });
    }

    fn spawn_insight_extraction(&self) {
        let s = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(
                s.config.insight_extraction_interval_secs,
            ));
            let mut failures: u32 = 0;
            loop {
                tokio::select! {
                    _ = s.cancel.cancelled() => { info!("Insight extraction task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if failures >= MAX_FAILURES {
                    let backoff =
                        compute_backoff(s.config.insight_extraction_interval_secs, failures);
                    warn!(
                        task = "insight_extraction",
                        failures, "Circuit breaker: backing off"
                    );
                    tokio::time::sleep(backoff).await;
                }
                if !s
                    .check_budget(
                        TaskType::InsightExtraction,
                        s.config.insight_extraction_batch_size as u64,
                    )
                    .await
                {
                    continue;
                }
                let start_time = std::time::Instant::now();
                match run_insight_extraction(&s.stores, s.config.insight_extraction_batch_size)
                    .await
                {
                    Ok(count) => {
                        failures = 0;
                        s.record_processed(TaskType::InsightExtraction, count as u64)
                            .await;
                        s.state.write().await.last_insight_extraction = Some(chrono::Utc::now());
                    }
                    Err(e) => {
                        failures += 1;
                        error!(task = "insight_extraction", error = %e, failures, "Task failed");
                        s.state.write().await.errors += 1;
                    }
                }
                check_time_budget(
                    start_time,
                    s.config.max_task_duration_ms,
                    "Insight extraction",
                );
            }
        });
    }

    fn spawn_prediction_validation(&self) {
        let s = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(
                s.config.prediction_validation_interval_secs,
            ));
            let mut failures: u32 = 0;
            loop {
                tokio::select! {
                    _ = s.cancel.cancelled() => { info!("Prediction validation task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if failures >= MAX_FAILURES {
                    let backoff =
                        compute_backoff(s.config.prediction_validation_interval_secs, failures);
                    warn!(
                        task = "prediction_validation",
                        failures, "Circuit breaker: backing off"
                    );
                    tokio::time::sleep(backoff).await;
                }
                if !s
                    .check_budget(
                        TaskType::PredictionValidation,
                        s.config.prediction_validation_batch_size as u64,
                    )
                    .await
                {
                    continue;
                }
                let start_time = std::time::Instant::now();
                match run_prediction_validation(&s.stores, &s.config).await {
                    Ok(count) => {
                        failures = 0;
                        s.record_processed(TaskType::PredictionValidation, count as u64)
                            .await;
                        s.state.write().await.last_prediction_validation = Some(chrono::Utc::now());
                    }
                    Err(e) => {
                        failures += 1;
                        error!(task = "prediction_validation", error = %e, failures, "Task failed");
                        s.state.write().await.errors += 1;
                    }
                }
                check_time_budget(
                    start_time,
                    s.config.max_task_duration_ms,
                    "Prediction validation",
                );
            }
        });
    }

    fn spawn_memory_promotion(&self) {
        let s = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(s.config.memory_promotion_interval_secs));
            let mut failures: u32 = 0;
            loop {
                tokio::select! {
                    _ = s.cancel.cancelled() => { info!("Memory promotion task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if failures >= MAX_FAILURES {
                    let backoff =
                        compute_backoff(s.config.memory_promotion_interval_secs, failures);
                    warn!(
                        task = "memory_promotion",
                        failures, "Circuit breaker: backing off"
                    );
                    tokio::time::sleep(backoff).await;
                }
                if !s.check_budget(TaskType::MemoryPromotion, 100).await {
                    continue;
                }
                match run_memory_promotion(
                    &s.stores,
                    s.config.working_to_episodic_rehearsal,
                    s.config.episodic_to_semantic_rehearsal,
                )
                .await
                {
                    Ok(count) => {
                        failures = 0;
                        s.record_processed(TaskType::MemoryPromotion, count as u64)
                            .await;
                    }
                    Err(e) => {
                        failures += 1;
                        error!(task = "memory_promotion", error = %e, failures, "Task failed");
                        s.state.write().await.errors += 1;
                    }
                }
            }
        });
    }
}

/// Compute exponential backoff duration, capped at MAX_BACKOFF_SECS
pub(crate) fn compute_backoff(base_interval_secs: u64, failures: u32) -> Duration {
    let backoff = Duration::from_secs(base_interval_secs * BACKOFF_MULTIPLIER.pow(failures.min(5)));
    backoff.min(Duration::from_secs(MAX_BACKOFF_SECS))
}

/// Log a warning if the task exceeded its time budget
pub(crate) fn check_time_budget(start: std::time::Instant, max_ms: u64, task_name: &str) {
    if start.elapsed().as_millis() > max_ms as u128 {
        warn!("{} task exceeded time budget", task_name);
    }
}
