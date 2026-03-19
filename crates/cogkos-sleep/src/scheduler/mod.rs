//! Sleep-time task scheduler
//!
//! Coordinates event-driven and periodic tasks for knowledge evolution.
//!
//! Task Types:
//! - Event-driven: Conflict detection (on write)
//! - Periodic: Consolidation (every 6h), Decay (daily), Health check (hourly)

mod core;
mod tasks;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

// Re-export all public items
pub use self::core::Scheduler;

/// Task types for budget allocation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskType {
    /// Event-driven conflict detection (on write)
    ConflictDetection,
    /// Event-driven consolidation (triggered by novel knowledge from ingest)
    ConsolidationEventDriven,
    /// Periodic consolidation (every 6h)
    Consolidation,
    /// Periodic decay (daily)
    Decay,
    /// Periodic health check (hourly)
    HealthCheck,
    /// Periodic conflict detection
    ConflictDetectionPeriodic,
    /// Confidence boost for similar knowledge (triggered by Sleep-time scheduler)
    ConfidenceBoost,
    /// Working memory GC (expire old working/episodic claims)
    MemoryGc,
    /// Memory layer promotion (working → episodic → semantic)
    MemoryPromotion,
}

impl TaskType {
    pub fn name(&self) -> &'static str {
        match self {
            TaskType::ConflictDetection => "conflict_detection",
            TaskType::ConsolidationEventDriven => "consolidation_event_driven",
            TaskType::Consolidation => "consolidation",
            TaskType::Decay => "decay",
            TaskType::HealthCheck => "health_check",
            TaskType::ConflictDetectionPeriodic => "conflict_detection_periodic",
            TaskType::ConfidenceBoost => "confidence_boost",
            TaskType::MemoryGc => "memory_gc",
            TaskType::MemoryPromotion => "memory_promotion",
        }
    }
}

/// Scheduler configuration
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Consolidation interval in seconds
    pub consolidation_interval_secs: u64,
    /// Event-driven consolidation interval in seconds (for PendingAggregation claims)
    pub consolidation_event_driven_interval_secs: u64,
    /// Decay interval in seconds
    pub decay_interval_secs: u64,
    /// Health check interval in seconds
    pub health_check_interval_secs: u64,
    /// Conflict detection interval in seconds
    pub conflict_interval_secs: u64,
    /// Confidence boost interval in seconds
    pub confidence_boost_interval_secs: u64,
    /// Conflict detection batch size
    pub conflict_batch_size: usize,
    /// Confidence boost batch size
    pub confidence_boost_batch_size: usize,
    /// Confidence boost factor (how much to boost each time)
    pub confidence_boost_factor: f64,
    /// Memory GC interval in seconds
    pub memory_gc_interval_secs: u64,
    /// Memory promotion interval in seconds
    pub memory_promotion_interval_secs: u64,
    /// Min rehearsal count to promote working → episodic
    pub working_to_episodic_rehearsal: u64,
    /// Min rehearsal count to promote episodic → semantic
    pub episodic_to_semantic_rehearsal: u64,
    /// Whether to enable periodic tasks
    pub enable_periodic: bool,
    /// Maximum claims to process across all tasks per hour (budget)
    pub max_claims_per_hour: u64,
    /// Maximum time in milliseconds for a single background task cycle
    pub max_task_duration_ms: u64,
    /// Budget allocation percentages per task type (must sum to 100)
    /// Key is task type name, value is percentage (0-100)
    pub task_budget_percentages: HashMap<String, u8>,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        let mut task_budget_percentages = HashMap::new();
        // Default budget allocation:
        // - Consolidation Event-Driven: 20% (triggered by novel knowledge)
        // - Consolidation: 15% (periodic)
        // - Decay: 15%
        // - Conflict Detection Periodic: 15%
        // - Conflict Detection (event-driven): 5%
        // - Confidence Boost: 10% (for similar knowledge aggregation)
        // - Memory GC: 5% (expire working/episodic claims)
        // - Memory Promotion: 5% (promote across layers)
        // - Health Check: 5% (minimal work)
        task_budget_percentages.insert(TaskType::ConsolidationEventDriven.name().to_string(), 20);
        task_budget_percentages.insert(TaskType::Consolidation.name().to_string(), 15);
        task_budget_percentages.insert(TaskType::Decay.name().to_string(), 15);
        task_budget_percentages.insert(TaskType::ConflictDetectionPeriodic.name().to_string(), 10);
        task_budget_percentages.insert(TaskType::ConflictDetection.name().to_string(), 5);
        task_budget_percentages.insert(TaskType::ConfidenceBoost.name().to_string(), 10);
        task_budget_percentages.insert(TaskType::MemoryGc.name().to_string(), 5);
        task_budget_percentages.insert(TaskType::MemoryPromotion.name().to_string(), 5);
        task_budget_percentages.insert(TaskType::HealthCheck.name().to_string(), 5);

        Self {
            consolidation_interval_secs: 6 * 3600,         // 6 hours
            consolidation_event_driven_interval_secs: 300, // 5 minutes
            decay_interval_secs: 24 * 3600,                // 24 hours
            health_check_interval_secs: 3600,              // 1 hour
            conflict_interval_secs: 2 * 3600,              // 2 hours
            confidence_boost_interval_secs: 1800,          // 30 minutes
            conflict_batch_size: 100,
            confidence_boost_batch_size: 50,
            confidence_boost_factor: 0.05,
            memory_gc_interval_secs: 1800,        // 30 minutes
            memory_promotion_interval_secs: 3600, // 1 hour
            working_to_episodic_rehearsal: 3,     // 3 recalls to promote
            episodic_to_semantic_rehearsal: 5,    // 5 recalls to promote
            enable_periodic: true,
            max_claims_per_hour: 10000,
            max_task_duration_ms: 300_000, // 5 minutes
            task_budget_percentages,
        }
    }
}

/// Scheduler state
#[derive(Debug, Clone)]
pub struct SchedulerState {
    pub is_running: bool,
    pub last_consolidation: Option<chrono::DateTime<chrono::Utc>>,
    pub last_decay: Option<chrono::DateTime<chrono::Utc>>,
    pub last_conflict_detection: Option<chrono::DateTime<chrono::Utc>>,
    pub last_confidence_boost: Option<chrono::DateTime<chrono::Utc>>,
    pub tasks_processed: u64,
    pub total_claims_processed: u64,
    pub errors: u64,
    pub current_budget_usage: u64,
    /// Per-task-type budget usage
    pub task_budget_usage: HashMap<String, u64>,
    pub last_budget_reset: chrono::DateTime<chrono::Utc>,
}

impl Default for SchedulerState {
    fn default() -> Self {
        Self {
            is_running: false,
            last_consolidation: None,
            last_decay: None,
            last_conflict_detection: None,
            last_confidence_boost: None,
            tasks_processed: 0,
            total_claims_processed: 0,
            errors: 0,
            current_budget_usage: 0,
            task_budget_usage: HashMap::new(),
            last_budget_reset: chrono::Utc::now(),
        }
    }
}

// Re-export for convenience
mod consolidation {}
