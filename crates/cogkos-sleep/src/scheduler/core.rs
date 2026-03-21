//! Scheduler struct and core implementation
//!
//! Periodic task spawn blocks are in spawn_periodic.rs to keep files under 500 lines.

use crate::conflict::{ConflictDetectionConfig, detect_conflicts};
use cogkos_core::Result;
use cogkos_core::evolution::engine::{EvolutionConfig, EvolutionEngine};
use cogkos_core::models::EpistemicClaim;
use cogkos_store::Stores;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::{SchedulerConfig, SchedulerState, TaskType};

/// Main scheduler
pub struct Scheduler {
    pub(crate) stores: Arc<Stores>,
    pub(crate) config: SchedulerConfig,
    pub(crate) state: Arc<RwLock<SchedulerState>>,
    pub(crate) evolution: Arc<RwLock<EvolutionEngine>>,
    pub(crate) cancel: tokio_util::sync::CancellationToken,
    /// Prediction history store — only used by scheduler tasks, not by MCP handlers.
    pub(crate) prediction_history: Option<Arc<dyn cogkos_store::PredictionHistoryStore>>,
}

impl Scheduler {
    pub fn new(stores: Arc<Stores>, config: SchedulerConfig) -> Self {
        Self {
            stores,
            config,
            state: Arc::new(RwLock::new(SchedulerState::default())),
            evolution: Arc::new(RwLock::new(
                EvolutionEngine::new(EvolutionConfig::default()),
            )),
            cancel: tokio_util::sync::CancellationToken::new(),
            prediction_history: None,
        }
    }

    /// Set the prediction history store for recording prediction errors.
    pub fn with_prediction_history(
        mut self,
        store: Arc<dyn cogkos_store::PredictionHistoryStore>,
    ) -> Self {
        self.prediction_history = Some(store);
        self
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

        // Spawn all periodic tasks (implementation in spawn_periodic.rs)
        self.spawn_periodic_tasks();
    }

    /// Helper to clone scheduler for task spawns
    pub(crate) fn clone_instance(&self) -> Self {
        Self {
            stores: self.stores.clone(),
            config: self.config.clone(),
            state: self.state.clone(),
            evolution: self.evolution.clone(),
            cancel: self.cancel.clone(),
            prediction_history: self.prediction_history.clone(),
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
