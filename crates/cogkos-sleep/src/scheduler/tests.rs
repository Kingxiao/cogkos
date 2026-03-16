use super::*;
use cogkos_store::Stores;
use std::sync::Arc;

#[test]
fn task_type_names() {
    assert_eq!(TaskType::ConflictDetection.name(), "conflict_detection");
    assert_eq!(TaskType::Consolidation.name(), "consolidation");
    assert_eq!(TaskType::Decay.name(), "decay");
    assert_eq!(TaskType::HealthCheck.name(), "health_check");
    assert_eq!(
        TaskType::ConflictDetectionPeriodic.name(),
        "conflict_detection_periodic"
    );
    assert_eq!(TaskType::ConfidenceBoost.name(), "confidence_boost");
    assert_eq!(
        TaskType::ConsolidationEventDriven.name(),
        "consolidation_event_driven"
    );
}

#[test]
fn scheduler_config_defaults() {
    let cfg = SchedulerConfig::default();
    assert_eq!(cfg.consolidation_interval_secs, 6 * 3600);
    assert_eq!(cfg.decay_interval_secs, 24 * 3600);
    assert_eq!(cfg.health_check_interval_secs, 3600);
    assert_eq!(cfg.conflict_interval_secs, 2 * 3600);
    assert_eq!(cfg.confidence_boost_interval_secs, 1800);
    assert_eq!(cfg.conflict_batch_size, 100);
    assert!(cfg.enable_periodic);
    assert_eq!(cfg.max_claims_per_hour, 10000);
    assert_eq!(cfg.max_task_duration_ms, 300_000);
}

#[test]
fn scheduler_config_budget_percentages_sum_to_90() {
    // Budget percentages intentionally sum to 90 (leaving 10% unallocated headroom)
    let cfg = SchedulerConfig::default();
    let total: u8 = cfg.task_budget_percentages.values().sum();
    assert_eq!(total, 90, "Budget percentages should sum to 90");
}

#[test]
fn scheduler_state_defaults() {
    let state = SchedulerState::default();
    assert!(!state.is_running);
    assert!(state.last_consolidation.is_none());
    assert!(state.last_decay.is_none());
    assert!(state.last_conflict_detection.is_none());
    assert!(state.last_confidence_boost.is_none());
    assert_eq!(state.tasks_processed, 0);
    assert_eq!(state.total_claims_processed, 0);
    assert_eq!(state.errors, 0);
    assert_eq!(state.current_budget_usage, 0);
}

fn test_stores() -> Arc<Stores> {
    use cogkos_core::audit::InMemoryAuditStore;
    use cogkos_store::*;
    Arc::new(Stores::new(
        Arc::new(InMemoryClaimStore::new()),
        Arc::new(InMemoryVectorStore::new()),
        Arc::new(InMemoryGraphStore::new()),
        Arc::new(InMemoryCacheStore::new()),
        Arc::new(InMemoryFeedbackStore::new()),
        Arc::new(cogkos_store::s3::InMemoryObjectStore::new()),
        Arc::new(InMemoryAuthStore::new()),
        Arc::new(InMemoryGapStore::new()),
        Arc::new(InMemoryAuditStore::new(1000)),
        Arc::new(InMemorySubscriptionStore::new()),
    ))
}

#[tokio::test]
async fn scheduler_check_budget_allows_within_limit() {
    let stores = test_stores();
    let cfg = SchedulerConfig::default();
    let scheduler = Scheduler::new(stores, cfg);

    // Consolidation gets 15% of 10000 = 1500
    assert!(scheduler.check_budget(TaskType::Consolidation, 100).await);
}

#[tokio::test]
async fn scheduler_check_budget_rejects_over_task_limit() {
    let stores = test_stores();
    let mut cfg = SchedulerConfig::default();
    cfg.max_claims_per_hour = 1000;
    // HealthCheck gets 5% = 50
    let scheduler = Scheduler::new(stores, cfg);

    assert!(!scheduler.check_budget(TaskType::HealthCheck, 51).await);
}

#[tokio::test]
async fn scheduler_check_budget_rejects_over_global_limit() {
    let stores = test_stores();
    let mut cfg = SchedulerConfig::default();
    cfg.max_claims_per_hour = 100;
    let scheduler = Scheduler::new(stores, cfg);

    assert!(!scheduler.check_budget(TaskType::Consolidation, 101).await);
}

#[tokio::test]
async fn scheduler_record_processed_updates_state() {
    let stores = test_stores();
    let cfg = SchedulerConfig::default();
    let scheduler = Scheduler::new(stores, cfg);

    scheduler
        .record_processed(TaskType::Consolidation, 50)
        .await;
    let state = scheduler.get_state().await;

    assert_eq!(state.current_budget_usage, 50);
    assert_eq!(state.total_claims_processed, 50);
    assert_eq!(state.tasks_processed, 1);
    assert_eq!(
        state.task_budget_usage.get("consolidation").copied(),
        Some(50)
    );
}

#[tokio::test]
async fn scheduler_record_processed_accumulates() {
    let stores = test_stores();
    let cfg = SchedulerConfig::default();
    let scheduler = Scheduler::new(stores, cfg);

    scheduler
        .record_processed(TaskType::Consolidation, 10)
        .await;
    scheduler.record_processed(TaskType::Decay, 20).await;
    scheduler
        .record_processed(TaskType::Consolidation, 30)
        .await;

    let state = scheduler.get_state().await;
    assert_eq!(state.current_budget_usage, 60);
    assert_eq!(state.tasks_processed, 3);
    assert_eq!(
        state.task_budget_usage.get("consolidation").copied(),
        Some(40)
    );
    assert_eq!(state.task_budget_usage.get("decay").copied(), Some(20));
}

#[tokio::test]
async fn scheduler_start_stop() {
    let stores = test_stores();
    let mut cfg = SchedulerConfig::default();
    cfg.enable_periodic = false; // Don't spawn background tasks in test
    let scheduler = Scheduler::new(stores, cfg);

    scheduler.start().await;
    let state = scheduler.get_state().await;
    assert!(state.is_running);

    scheduler.stop().await;
    let state = scheduler.get_state().await;
    assert!(!state.is_running);
}

#[tokio::test]
async fn scheduler_cancellation_token() {
    let stores = test_stores();
    let cfg = SchedulerConfig::default();
    let scheduler = Scheduler::new(stores, cfg);

    let token = scheduler.cancellation_token();
    assert!(!token.is_cancelled());
    scheduler.stop().await;
    assert!(token.is_cancelled());
}
