//! Scheduler struct and implementation

use crate::conflict::{ConflictDetectionConfig, detect_conflicts, detect_conflicts_periodic};
use crate::consolidate::ConsolidationConfig;
use crate::decay::DecayConfig;
use cogkos_core::Result;
use cogkos_core::evolution::engine::{EvolutionConfig, EvolutionEngine};
use cogkos_core::models::{AnomalySignals, EpistemicClaim, EvolutionMode};
use cogkos_store::Stores;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{error, info, warn};

use super::tasks::{
    run_confidence_boost, run_consolidation, run_decay, run_health_check, run_memory_gc,
    run_memory_promotion, run_pending_aggregation,
};
use super::{SchedulerConfig, SchedulerState, TaskType};

/// Main scheduler
pub struct Scheduler {
    stores: Arc<Stores>,
    config: SchedulerConfig,
    state: Arc<RwLock<SchedulerState>>,
    evolution: Arc<RwLock<EvolutionEngine>>,
    cancel: tokio_util::sync::CancellationToken,
}

impl Scheduler {
    pub fn new(stores: Arc<Stores>, config: SchedulerConfig) -> Self {
        Self {
            stores,
            config,
            state: Arc::new(RwLock::new(SchedulerState::default())),
            evolution: Arc::new(RwLock::new(EvolutionEngine::new(EvolutionConfig::default()))),
            cancel: tokio_util::sync::CancellationToken::new(),
        }
    }

    /// Check if we have enough budget to process claims for a specific task type
    /// Returns true if budget is available, false if exceeded
    pub(crate) async fn check_budget(&self, task_type: TaskType, claims_to_process: u64) -> bool {
        let mut state = self.state.write().await;
        let now = chrono::Utc::now();

        // Reset budget if more than 1 hour passed
        if (now - state.last_budget_reset).num_hours() >= 1 {
            state.current_budget_usage = 0;
            state.task_budget_usage.clear();
            state.last_budget_reset = now;
        }

        let task_name = task_type.name();

        // Calculate task-specific budget limit based on percentage
        let task_limit = self
            .config
            .task_budget_percentages
            .get(task_name)
            .map(|&pct| (self.config.max_claims_per_hour as f64 * pct as f64 / 100.0) as u64)
            .unwrap_or(self.config.max_claims_per_hour);

        // Check task-specific budget
        let current_task_usage = state.task_budget_usage.get(task_name).copied().unwrap_or(0);
        if current_task_usage + claims_to_process > task_limit {
            warn!(
                task_type = task_name,
                usage = current_task_usage,
                requested = claims_to_process,
                limit = task_limit,
                "Task-specific budget exceeded"
            );
            return false;
        }

        // Also check global budget
        if state.current_budget_usage + claims_to_process > self.config.max_claims_per_hour {
            warn!(
                global_usage = state.current_budget_usage,
                requested = claims_to_process,
                limit = self.config.max_claims_per_hour,
                "Global budget exceeded"
            );
            return false;
        }

        true
    }

    /// Record processed claims in budget for a specific task type
    pub(crate) async fn record_processed(&self, task_type: TaskType, count: u64) {
        let mut state = self.state.write().await;
        let task_name = task_type.name();

        state.current_budget_usage += count;
        state.total_claims_processed += count;
        state.tasks_processed += 1;

        // Update task-specific budget usage
        let current = state
            .task_budget_usage
            .entry(task_name.to_string())
            .or_insert(0);
        *current += count;
    }

    /// Start the sleep-time scheduler
    pub async fn start(&self) {
        info!("Starting sleep-time scheduler");

        let mut state = self.state.write().await;
        state.is_running = true;
        drop(state);

        if !self.config.enable_periodic {
            info!("Periodic tasks disabled");
            return;
        }

        // Event-driven consolidation: every 5 minutes (for PendingAggregation claims)
        // This processes novel knowledge from ingest pipeline that needs Sleep-time aggregation
        let s_pending = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(
                s_pending.config.consolidation_event_driven_interval_secs,
            ));
            loop {
                tokio::select! {
                    _ = s_pending.cancel.cancelled() => {
                        info!("PendingAggregation task shutting down");
                        break;
                    }
                    _ = ticker.tick() => {}
                }
                if s_pending
                    .check_budget(
                        TaskType::ConsolidationEventDriven,
                        s_pending.config.conflict_batch_size as u64,
                    )
                    .await
                {
                    let start_time = std::time::Instant::now();
                    let consolidation_config = ConsolidationConfig {
                        batch_size: s_pending.config.conflict_batch_size,
                        ..ConsolidationConfig::default()
                    };

                    match run_pending_aggregation(&s_pending.stores, &consolidation_config).await {
                        Ok(count) => {
                            s_pending
                                .record_processed(TaskType::ConsolidationEventDriven, count as u64)
                                .await;
                            info!(processed = count, "Processed PendingAggregation claims");
                        }
                        Err(e) => {
                            error!(error = %e, "PendingAggregation processing failed");
                            let mut state = s_pending.state.write().await;
                            state.errors += 1;
                        }
                    }

                    if start_time.elapsed().as_millis()
                        > s_pending.config.max_task_duration_ms as u128
                    {
                        warn!("PendingAggregation task exceeded time budget");
                    }
                }
            }
        });

        // Consolidation: every 6 hours
        let s_consolidation = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(
                s_consolidation.config.consolidation_interval_secs,
            ));
            loop {
                tokio::select! {
                    _ = s_consolidation.cancel.cancelled() => { info!("Consolidation task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if s_consolidation
                    .check_budget(
                        TaskType::Consolidation,
                        s_consolidation.config.conflict_batch_size as u64,
                    )
                    .await
                {
                    let start_time = std::time::Instant::now();
                    let consolidation_config = ConsolidationConfig {
                        batch_size: s_consolidation.config.conflict_batch_size,
                        ..ConsolidationConfig::default()
                    };

                    match run_consolidation(&s_consolidation.stores, &consolidation_config).await {
                        Ok(count) => {
                            s_consolidation
                                .record_processed(TaskType::Consolidation, count as u64)
                                .await;
                            let mut state = s_consolidation.state.write().await;
                            state.last_consolidation = Some(chrono::Utc::now());

                            // Tick evolution engine with anomaly signals
                            let signals = AnomalySignals {
                                prediction_error_streak: 0, // TODO: wire up from PredictionHistoryStore
                                conflict_density_pct: 0.0,  // TODO: compute from recent conflicts
                                cache_hit_rate_trend: 0.0,  // TODO: compute from cache stats
                            };
                            let mut engine = s_consolidation.evolution.write().await;
                            engine.tick(signals);
                            if engine.state().mode == EvolutionMode::ParadigmShift {
                                warn!(
                                    anomaly_counter = engine.state().anomaly_counter,
                                    "Evolution engine triggered ParadigmShift mode — Phase 3 will implement shift logic"
                                );
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "Consolidation failed");
                            let mut state = s_consolidation.state.write().await;
                            state.errors += 1;
                        }
                    }

                    if start_time.elapsed().as_millis()
                        > s_consolidation.config.max_task_duration_ms as u128
                    {
                        warn!("Consolidation task exceeded time budget");
                    }
                }
            }
        });

        // Decay: every 24 hours
        let s_decay = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(s_decay.config.decay_interval_secs));
            loop {
                tokio::select! {
                    _ = s_decay.cancel.cancelled() => { info!("Decay task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if s_decay
                    .check_budget(TaskType::Decay, s_decay.config.conflict_batch_size as u64)
                    .await
                {
                    let start_time = std::time::Instant::now();
                    let decay_config = DecayConfig {
                        batch_size: s_decay.config.conflict_batch_size,
                        ..DecayConfig::default()
                    };

                    match run_decay(&s_decay.stores, &decay_config).await {
                        Ok(count) => {
                            s_decay
                                .record_processed(TaskType::Decay, count as u64)
                                .await;
                            let mut state = s_decay.state.write().await;
                            state.last_decay = Some(chrono::Utc::now());
                        }
                        Err(e) => {
                            error!(error = %e, "Decay failed");
                            let mut state = s_decay.state.write().await;
                            state.errors += 1;
                        }
                    }

                    if start_time.elapsed().as_millis()
                        > s_decay.config.max_task_duration_ms as u128
                    {
                        warn!("Decay task exceeded time budget");
                    }
                }
            }
        });

        // Health check: every hour
        let s_health = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(
                s_health.config.health_check_interval_secs,
            ));
            loop {
                tokio::select! {
                    _ = s_health.cancel.cancelled() => { info!("Health check task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                // Health check uses minimal budget (just connectivity checks)
                if s_health.check_budget(TaskType::HealthCheck, 10).await {
                    if let Err(e) = run_health_check(&s_health.stores).await {
                        error!(error = %e, "Health check failed");
                        let mut state = s_health.state.write().await;
                        state.errors += 1;
                    } else {
                        s_health.record_processed(TaskType::HealthCheck, 1).await;
                    }
                }
            }
        });

        // Conflict detection periodic
        let s_conflict = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(
                s_conflict.config.conflict_interval_secs,
            ));
            loop {
                tokio::select! {
                    _ = s_conflict.cancel.cancelled() => { info!("Conflict detection task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if s_conflict
                    .check_budget(
                        TaskType::ConflictDetectionPeriodic,
                        s_conflict.config.conflict_batch_size as u64,
                    )
                    .await
                {
                    let start_time = std::time::Instant::now();
                    // Process default tenant - in real impl would iterate all tenants
                    match detect_conflicts_periodic(
                        &s_conflict.stores,
                        "default",
                        s_conflict.config.conflict_batch_size,
                    )
                    .await
                    {
                        Ok(conflicts) => {
                            s_conflict
                                .record_processed(
                                    TaskType::ConflictDetectionPeriodic,
                                    conflicts.len() as u64,
                                )
                                .await;
                            let mut state = s_conflict.state.write().await;
                            state.last_conflict_detection = Some(chrono::Utc::now());
                        }
                        Err(e) => {
                            error!(error = %e, "Periodic conflict detection failed");
                            let mut state = s_conflict.state.write().await;
                            state.errors += 1;
                        }
                    }

                    if start_time.elapsed().as_millis()
                        > s_conflict.config.max_task_duration_ms as u128
                    {
                        warn!("Conflict detection task exceeded time budget");
                    }
                }
            }
        });

        // Confidence boost for similar knowledge (every 30 minutes)
        let s_confidence_boost = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(
                s_confidence_boost.config.confidence_boost_interval_secs,
            ));
            loop {
                tokio::select! {
                    _ = s_confidence_boost.cancel.cancelled() => { info!("Confidence boost task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if s_confidence_boost
                    .check_budget(
                        TaskType::ConfidenceBoost,
                        s_confidence_boost.config.confidence_boost_batch_size as u64,
                    )
                    .await
                {
                    let start_time = std::time::Instant::now();

                    // Process default tenant - in real impl would iterate all tenants
                    match run_confidence_boost(
                        &s_confidence_boost.stores,
                        "default",
                        s_confidence_boost.config.confidence_boost_batch_size,
                        s_confidence_boost.config.confidence_boost_factor,
                    )
                    .await
                    {
                        Ok(count) => {
                            s_confidence_boost
                                .record_processed(TaskType::ConfidenceBoost, count as u64)
                                .await;
                            let mut state = s_confidence_boost.state.write().await;
                            state.last_confidence_boost = Some(chrono::Utc::now());
                        }
                        Err(e) => {
                            error!(error = %e, "Confidence boost task failed");
                            let mut state = s_confidence_boost.state.write().await;
                            state.errors += 1;
                        }
                    }

                    if start_time.elapsed().as_millis()
                        > s_confidence_boost.config.max_task_duration_ms as u128
                    {
                        warn!("Confidence boost task exceeded time budget");
                    }
                }
            }
        });

        // Memory GC: expire old working/episodic claims
        let s_gc = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(s_gc.config.memory_gc_interval_secs));
            loop {
                tokio::select! {
                    _ = s_gc.cancel.cancelled() => { info!("Memory GC task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if s_gc.check_budget(TaskType::MemoryGc, 100).await {
                    match run_memory_gc(&s_gc.stores).await {
                        Ok(count) => {
                            s_gc.record_processed(TaskType::MemoryGc, count as u64).await;
                        }
                        Err(e) => {
                            error!(error = %e, "Memory GC failed");
                            let mut state = s_gc.state.write().await;
                            state.errors += 1;
                        }
                    }
                }
            }
        });

        // Memory promotion: working → episodic → semantic
        let s_promo = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker =
                interval(Duration::from_secs(s_promo.config.memory_promotion_interval_secs));
            loop {
                tokio::select! {
                    _ = s_promo.cancel.cancelled() => { info!("Memory promotion task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if s_promo.check_budget(TaskType::MemoryPromotion, 100).await {
                    match run_memory_promotion(
                        &s_promo.stores,
                        s_promo.config.working_to_episodic_rehearsal,
                        s_promo.config.episodic_to_semantic_rehearsal,
                    )
                    .await
                    {
                        Ok(count) => {
                            s_promo
                                .record_processed(TaskType::MemoryPromotion, count as u64)
                                .await;
                        }
                        Err(e) => {
                            error!(error = %e, "Memory promotion failed");
                            let mut state = s_promo.state.write().await;
                            state.errors += 1;
                        }
                    }
                }
            }
        });
    }

    /// Helper to clone scheduler for task spawns
    pub(crate) fn clone_instance(&self) -> Self {
        Self {
            stores: self.stores.clone(),
            config: self.config.clone(),
            state: self.state.clone(),
            evolution: self.evolution.clone(),
            cancel: self.cancel.clone(),
        }
    }

    /// Stop the scheduler -- cancels all background tasks
    pub async fn stop(&self) {
        info!("Stopping sleep-time scheduler");
        self.cancel.cancel();
        let mut state = self.state.write().await;
        state.is_running = false;
    }

    /// Get cancellation token (for external shutdown coordination)
    pub fn cancellation_token(&self) -> tokio_util::sync::CancellationToken {
        self.cancel.clone()
    }

    /// Get scheduler state
    pub async fn get_state(&self) -> SchedulerState {
        self.state.read().await.clone()
    }

    /// Trigger event-driven conflict detection for a new claim
    pub async fn on_claim_written(
        &self,
        claim: &EpistemicClaim,
    ) -> Result<Vec<cogkos_core::models::ConflictRecord>> {
        // Check budget for event-driven conflict detection
        if !self.check_budget(TaskType::ConflictDetection, 1).await {
            warn!(claim_id = %claim.id, "Budget exceeded for event-driven conflict detection, skipping");
            return Ok(vec![]);
        }

        info!(claim_id = %claim.id, "Event: claim written - running conflict detection");

        let config = ConflictDetectionConfig::default();
        let conflicts = detect_conflicts(&self.stores, claim, &config).await?;

        self.record_processed(TaskType::ConflictDetection, 1).await;

        Ok(conflicts)
    }
}
