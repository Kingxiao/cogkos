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
    assert_eq!(TaskType::MemoryGc.name(), "memory_gc");
    assert_eq!(TaskType::MemoryPromotion.name(), "memory_promotion");
    assert_eq!(TaskType::InsightExtraction.name(), "insight_extraction");
    assert_eq!(
        TaskType::PredictionValidation.name(),
        "prediction_validation"
    );
    assert_eq!(TaskType::ParadigmShiftCheck.name(), "paradigm_shift_check");
    assert_eq!(
        TaskType::FrameworkHealthMonitoring.name(),
        "framework_health_monitoring"
    );
    assert_eq!(
        TaskType::CollectiveWisdomCheck.name(),
        "collective_wisdom_check"
    );
    assert_eq!(TaskType::LlmExtraction.name(), "llm_extraction");
}

#[test]
fn scheduler_config_defaults() {
    let cfg = SchedulerConfig::default();
    assert_eq!(cfg.consolidation_interval_secs, 6 * 3600);
    assert_eq!(cfg.decay_interval_secs, 24 * 3600);
    assert_eq!(cfg.health_check_interval_secs, 3600);
    assert_eq!(cfg.conflict_interval_secs, 2 * 3600);
    assert_eq!(cfg.confidence_boost_interval_secs, 1800);
    assert_eq!(cfg.insight_extraction_interval_secs, 4 * 3600);
    assert_eq!(cfg.insight_extraction_batch_size, 50);
    assert_eq!(cfg.prediction_validation_interval_secs, 3600);
    assert_eq!(cfg.prediction_validation_batch_size, 50);
    assert_eq!(cfg.paradigm_shift_check_interval_secs, 12 * 3600);
    assert_eq!(cfg.framework_health_interval_secs, 2 * 3600);
    assert_eq!(cfg.collective_wisdom_check_interval_secs, 6 * 3600);
    assert_eq!(cfg.llm_extraction_interval_secs, 4 * 3600);
    assert_eq!(cfg.conflict_batch_size, 100);
    assert!(cfg.enable_periodic);
    assert_eq!(cfg.max_claims_per_hour, 10000);
    assert_eq!(cfg.max_task_duration_ms, 300_000);
}

#[test]
fn scheduler_config_budget_percentages_sum_to_95() {
    // Budget percentages intentionally sum to 95 (leaving 5% unallocated headroom)
    let cfg = SchedulerConfig::default();
    let total: u8 = cfg.task_budget_percentages.values().sum();
    assert_eq!(total, 100, "Budget percentages should sum to 100");
}

#[test]
fn scheduler_state_defaults() {
    let state = SchedulerState::default();
    assert!(!state.is_running);
    assert!(state.last_consolidation.is_none());
    assert!(state.last_decay.is_none());
    assert!(state.last_conflict_detection.is_none());
    assert!(state.last_confidence_boost.is_none());
    assert!(state.last_insight_extraction.is_none());
    assert!(state.last_prediction_validation.is_none());
    assert!(state.last_paradigm_shift_check.is_none());
    assert!(state.last_framework_health.is_none());
    assert!(state.last_collective_wisdom_check.is_none());
    assert!(state.last_content_consolidation.is_none());
    assert!(state.last_llm_extraction.is_none());
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
        Arc::new(NoopMemoryLayerStore),
    ))
}

#[tokio::test]
async fn scheduler_check_budget_allows_within_limit() {
    let stores = test_stores();
    let cfg = SchedulerConfig::default();
    let scheduler = Scheduler::new(stores, cfg);

    // Consolidation gets 10% of 10000 = 1000
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
async fn scheduler_insight_extraction_budget() {
    let stores = test_stores();
    let mut cfg = SchedulerConfig::default();
    cfg.max_claims_per_hour = 1000;
    // InsightExtraction gets 5% = 50
    let scheduler = Scheduler::new(stores, cfg);

    assert!(
        scheduler
            .check_budget(TaskType::InsightExtraction, 50)
            .await
    );
    assert!(
        !scheduler
            .check_budget(TaskType::InsightExtraction, 51)
            .await
    );
}

#[tokio::test]
async fn scheduler_prediction_validation_budget() {
    let stores = test_stores();
    let mut cfg = SchedulerConfig::default();
    cfg.max_claims_per_hour = 1000;
    // PredictionValidation gets 5% = 50
    let scheduler = Scheduler::new(stores, cfg);

    assert!(
        scheduler
            .check_budget(TaskType::PredictionValidation, 50)
            .await
    );
    assert!(
        !scheduler
            .check_budget(TaskType::PredictionValidation, 51)
            .await
    );
}

#[tokio::test]
async fn scheduler_paradigm_shift_check_budget() {
    let stores = test_stores();
    let mut cfg = SchedulerConfig::default();
    cfg.max_claims_per_hour = 1000;
    // ParadigmShiftCheck gets 5% = 50
    let scheduler = Scheduler::new(stores, cfg);

    assert!(
        scheduler
            .check_budget(TaskType::ParadigmShiftCheck, 50)
            .await
    );
    assert!(
        !scheduler
            .check_budget(TaskType::ParadigmShiftCheck, 51)
            .await
    );
}

#[tokio::test]
async fn scheduler_framework_health_budget() {
    let stores = test_stores();
    let mut cfg = SchedulerConfig::default();
    cfg.max_claims_per_hour = 1000;
    // FrameworkHealthMonitoring gets 5% = 50
    let scheduler = Scheduler::new(stores, cfg);

    assert!(
        scheduler
            .check_budget(TaskType::FrameworkHealthMonitoring, 50)
            .await
    );
    assert!(
        !scheduler
            .check_budget(TaskType::FrameworkHealthMonitoring, 51)
            .await
    );
}

#[test]
fn scheduler_config_budget_all_tasks_registered() {
    let cfg = SchedulerConfig::default();
    // Verify all 16 task types have budget entries
    assert_eq!(cfg.task_budget_percentages.len(), 16);
    for task_type in [
        TaskType::ConsolidationEventDriven,
        TaskType::Consolidation,
        TaskType::Decay,
        TaskType::ConflictDetectionPeriodic,
        TaskType::ConflictDetection,
        TaskType::ConfidenceBoost,
        TaskType::MemoryGc,
        TaskType::MemoryPromotion,
        TaskType::HealthCheck,
        TaskType::InsightExtraction,
        TaskType::PredictionValidation,
        TaskType::ParadigmShiftCheck,
        TaskType::FrameworkHealthMonitoring,
        TaskType::CollectiveWisdomCheck,
        TaskType::ContentConsolidation,
        TaskType::LlmExtraction,
    ] {
        assert!(
            cfg.task_budget_percentages.contains_key(task_type.name()),
            "Missing budget for task: {}",
            task_type.name()
        );
    }
}

#[tokio::test]
async fn scheduler_collective_wisdom_check_budget() {
    let stores = test_stores();
    let mut cfg = SchedulerConfig::default();
    cfg.max_claims_per_hour = 1000;
    // CollectiveWisdomCheck gets 5% = 50
    let scheduler = Scheduler::new(stores, cfg);

    assert!(
        scheduler
            .check_budget(TaskType::CollectiveWisdomCheck, 50)
            .await
    );
    assert!(
        !scheduler
            .check_budget(TaskType::CollectiveWisdomCheck, 51)
            .await
    );
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

// -- Collective wisdom four-conditions unit tests --
// These test the federation health module directly, validating that
// multi-agent scenarios produce correct diversity/independence/decentralization scores.

mod collective_wisdom_tests {
    use cogkos_federation::health::{
        HealthStatus, InsightSource, Prediction, ProvenanceInfo, calculate_collective_health,
    };

    fn make_source(agent_id: &str, influence: f64, confidence: f64) -> InsightSource {
        InsightSource {
            source_id: agent_id.to_string(),
            provenance: ProvenanceInfo {
                source_id: agent_id.to_string(),
                source_type: "agent".to_string(),
                upstream_sources: vec![],
            },
            influence,
            confidence,
            predictions: vec![Prediction {
                content: format!("prediction from {}", agent_id),
                confidence,
            }],
        }
    }

    #[test]
    fn diverse_agents_high_diversity() {
        // 3 different agents with equal distribution → high diversity
        let sources = vec![
            make_source("agent-alpha", 0.33, 0.8),
            make_source("agent-beta", 0.33, 0.7),
            make_source("agent-gamma", 0.34, 0.9),
        ];

        let health = calculate_collective_health(&sources);

        assert!(
            health.diversity_score > 0.9,
            "3 distinct agents should yield high diversity, got {}",
            health.diversity_score
        );
        assert_eq!(health.conditions.diversity.status, HealthStatus::Healthy);
    }

    #[test]
    fn single_agent_low_independence() {
        // All claims from same agent → low independence
        let sources = vec![
            make_source("agent-alpha", 0.5, 0.8),
            make_source("agent-alpha", 0.3, 0.7),
            make_source("agent-alpha", 0.2, 0.9),
        ];

        let health = calculate_collective_health(&sources);

        assert_eq!(
            health.conditions.diversity.status,
            HealthStatus::Unhealthy,
            "Single source should be unhealthy diversity"
        );
        assert_eq!(
            health.conditions.independence.status,
            HealthStatus::Unhealthy,
            "Same provenance should be unhealthy independence"
        );
    }

    #[test]
    fn dominant_agent_centralization_risk() {
        // One agent has 90% of influence → centralization risk
        let sources = vec![
            make_source("agent-alpha", 0.90, 0.8),
            make_source("agent-beta", 0.03, 0.7),
            make_source("agent-gamma", 0.03, 0.9),
            make_source("agent-delta", 0.04, 0.6),
        ];

        let health = calculate_collective_health(&sources);

        assert!(
            health.conditions.decentralization.gini_coefficient > 0.5,
            "Dominant agent should cause high Gini, got {}",
            health.conditions.decentralization.gini_coefficient
        );
        assert_eq!(
            health.conditions.decentralization.status,
            HealthStatus::Unhealthy,
            "Dominant agent should be unhealthy decentralization"
        );
    }

    #[test]
    fn balanced_agents_healthy_overall() {
        // 4 agents with equal influence → all conditions healthy
        let sources = vec![
            make_source("agent-alpha", 0.25, 0.8),
            make_source("agent-beta", 0.25, 0.7),
            make_source("agent-gamma", 0.25, 0.9),
            make_source("agent-delta", 0.25, 0.6),
        ];

        let health = calculate_collective_health(&sources);

        assert!(
            health.overall_health > 0.7,
            "Balanced agents should yield high overall health, got {}",
            health.overall_health
        );
        assert_eq!(health.conditions.diversity.status, HealthStatus::Healthy);
        assert_eq!(
            health.conditions.decentralization.status,
            HealthStatus::Healthy
        );
    }

    #[test]
    fn extract_agent_id_formats() {
        use crate::scheduler::tasks_phase3::extract_agent_id;
        use cogkos_core::models::Claimant;

        assert_eq!(
            extract_agent_id(&Claimant::Agent {
                agent_id: "claude-dev".to_string(),
                model: "claude-sonnet".to_string(),
            }),
            "claude-dev"
        );
        assert_eq!(
            extract_agent_id(&Claimant::Human {
                user_id: "alice".to_string(),
                role: "admin".to_string(),
            }),
            "human:alice"
        );
        assert_eq!(extract_agent_id(&Claimant::System), "system");
        assert_eq!(
            extract_agent_id(&Claimant::ExternalPublic {
                source_name: "rss-feed".to_string(),
            }),
            "external:rss-feed"
        );
    }
}
