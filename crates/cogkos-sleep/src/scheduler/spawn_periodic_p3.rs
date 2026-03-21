//! Phase 3 periodic task spawn implementations (paradigm shift, framework health, collective wisdom)

use std::time::Duration;
use tokio::time::interval;
use tracing::{error, info, warn};

use super::TaskType;
use super::core::Scheduler;
use super::spawn_periodic::{MAX_FAILURES, check_time_budget, compute_backoff};
use super::tasks_phase3::{
    run_collective_wisdom_check, run_framework_health_monitoring, run_paradigm_shift_check,
};

impl Scheduler {
    pub(crate) fn spawn_paradigm_shift_check(&self) {
        let s = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(
                s.config.paradigm_shift_check_interval_secs,
            ));
            let mut failures: u32 = 0;
            loop {
                tokio::select! {
                    _ = s.cancel.cancelled() => { info!("Paradigm shift check task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if failures >= MAX_FAILURES {
                    let backoff =
                        compute_backoff(s.config.paradigm_shift_check_interval_secs, failures);
                    warn!(
                        task = "paradigm_shift_check",
                        failures, "Circuit breaker: backing off"
                    );
                    tokio::time::sleep(backoff).await;
                }
                if !s.check_budget(TaskType::ParadigmShiftCheck, 10).await {
                    continue;
                }
                let start_time = std::time::Instant::now();
                match run_paradigm_shift_check(&s.stores, &s.config).await {
                    Ok(count) => {
                        failures = 0;
                        s.record_processed(TaskType::ParadigmShiftCheck, count as u64)
                            .await;
                        s.state.write().await.last_paradigm_shift_check = Some(chrono::Utc::now());
                    }
                    Err(e) => {
                        failures += 1;
                        error!(task = "paradigm_shift_check", error = %e, failures, "Task failed");
                        s.state.write().await.errors += 1;
                    }
                }
                check_time_budget(
                    start_time,
                    s.config.max_task_duration_ms,
                    "Paradigm shift check",
                );
            }
        });
    }

    pub(crate) fn spawn_framework_health(&self) {
        let s = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(s.config.framework_health_interval_secs));
            let mut failures: u32 = 0;
            loop {
                tokio::select! {
                    _ = s.cancel.cancelled() => { info!("Framework health monitoring task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if failures >= MAX_FAILURES {
                    let backoff =
                        compute_backoff(s.config.framework_health_interval_secs, failures);
                    warn!(
                        task = "framework_health",
                        failures, "Circuit breaker: backing off"
                    );
                    tokio::time::sleep(backoff).await;
                }
                if !s
                    .check_budget(TaskType::FrameworkHealthMonitoring, 10)
                    .await
                {
                    continue;
                }
                let start_time = std::time::Instant::now();
                match run_framework_health_monitoring(&s.stores, &s.config).await {
                    Ok(count) => {
                        failures = 0;
                        s.record_processed(TaskType::FrameworkHealthMonitoring, count as u64)
                            .await;
                        s.state.write().await.last_framework_health = Some(chrono::Utc::now());
                    }
                    Err(e) => {
                        failures += 1;
                        error!(task = "framework_health", error = %e, failures, "Task failed");
                        s.state.write().await.errors += 1;
                    }
                }
                check_time_budget(
                    start_time,
                    s.config.max_task_duration_ms,
                    "Framework health",
                );
            }
        });
    }

    pub(crate) fn spawn_collective_wisdom_check(&self) {
        let s = self.clone_instance();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(
                s.config.collective_wisdom_check_interval_secs,
            ));
            let mut failures: u32 = 0;
            loop {
                tokio::select! {
                    _ = s.cancel.cancelled() => { info!("Collective wisdom check task shutting down"); break; }
                    _ = ticker.tick() => {}
                }
                if failures >= MAX_FAILURES {
                    let backoff =
                        compute_backoff(s.config.collective_wisdom_check_interval_secs, failures);
                    warn!(
                        task = "collective_wisdom_check",
                        failures, "Circuit breaker: backing off"
                    );
                    tokio::time::sleep(backoff).await;
                }
                if !s.check_budget(TaskType::CollectiveWisdomCheck, 10).await {
                    continue;
                }
                let start_time = std::time::Instant::now();
                match run_collective_wisdom_check(&s.stores, &s.config).await {
                    Ok(count) => {
                        failures = 0;
                        s.record_processed(TaskType::CollectiveWisdomCheck, count as u64)
                            .await;
                        s.state.write().await.last_collective_wisdom_check =
                            Some(chrono::Utc::now());
                    }
                    Err(e) => {
                        failures += 1;
                        error!(task = "collective_wisdom_check", error = %e, failures, "Task failed");
                        s.state.write().await.errors += 1;
                    }
                }
                check_time_budget(
                    start_time,
                    s.config.max_task_duration_ms,
                    "Collective wisdom check",
                );
            }
        });
    }
}
