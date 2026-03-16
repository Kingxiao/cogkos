//! Retry and Circuit Breaker module
//!
//! Provides:
//! - Exponential backoff retry logic
//! - Circuit breaker state machine (closed, open, half-open)
//! - Failure counting and recovery detection

use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Circuit breaker states
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CircuitBreakerState {
    /// Normal operation, requests allowed
    Closed,
    /// Circuit open, requests blocked
    Open,
    /// Testing if service recovered
    HalfOpen,
}

/// Circuit breaker configuration
#[derive(Clone, Debug)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening circuit
    pub failure_threshold: usize,
    /// Number of successes needed to close circuit from half-open
    pub success_threshold: usize,
    /// Duration to wait before trying half-open
    pub timeout: Duration,
    /// Half-open request timeout
    pub half_open_timeout: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            timeout: Duration::from_secs(30),
            half_open_timeout: Duration::from_secs(10),
        }
    }
}

/// Circuit breaker for fault tolerance
#[derive(Clone)]
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: Arc<RwLock<CircuitBreakerState>>,
    failure_count: Arc<AtomicUsize>,
    success_count: Arc<AtomicUsize>,
    last_failure_time: Arc<RwLock<Option<Instant>>>,
    last_state_change: Arc<RwLock<Instant>>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with default config
    pub fn new() -> Self {
        Self::with_config(CircuitBreakerConfig::default())
    }

    /// Create a circuit breaker with custom config
    pub fn with_config(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(CircuitBreakerState::Closed)),
            failure_count: Arc::new(AtomicUsize::new(0)),
            success_count: Arc::new(AtomicUsize::new(0)),
            last_failure_time: Arc::new(RwLock::new(None)),
            last_state_change: Arc::new(RwLock::new(Instant::now())),
        }
    }

    /// Check if request is allowed
    pub fn is_available(&self) -> bool {
        let state = self.state.read().clone();
        match state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open => {
                // Check if timeout has passed to transition to half-open
                let last_change = *self.last_state_change.read();
                if last_change.elapsed() >= self.config.timeout {
                    self.transition_to_half_open();
                    true
                } else {
                    false
                }
            }
            CircuitBreakerState::HalfOpen => true,
        }
    }

    /// Record a successful request
    pub fn record_success(&self) {
        let state = self.state.read().clone();
        match state {
            CircuitBreakerState::Closed => {
                self.failure_count.store(0, Ordering::SeqCst);
            }
            CircuitBreakerState::HalfOpen => {
                let successes = self.success_count.fetch_add(1, Ordering::SeqCst) + 1;
                if successes >= self.config.success_threshold {
                    self.transition_to_closed();
                }
            }
            CircuitBreakerState::Open => {
                // Should not happen, but handle gracefully
            }
        }
    }

    /// Record a failed request
    pub fn record_failure(&self) {
        let failures = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
        *self.last_failure_time.write() = Some(Instant::now());

        if failures >= self.config.failure_threshold {
            self.transition_to_open();
        }
    }

    /// Get current state
    pub fn get_state(&self) -> CircuitBreakerState {
        (*self.state.read()).clone()
    }

    fn transition_to_open(&self) {
        let mut state = self.state.write();
        if *state != CircuitBreakerState::Open {
            *state = CircuitBreakerState::Open;
            *self.last_state_change.write() = Instant::now();
            tracing::warn!("Circuit breaker opened");
        }
    }

    fn transition_to_half_open(&self) {
        let mut state = self.state.write();
        if *state == CircuitBreakerState::Open {
            *state = CircuitBreakerState::HalfOpen;
            *self.last_state_change.write() = Instant::now();
            self.success_count.store(0, Ordering::SeqCst);
            tracing::info!("Circuit breaker half-open");
        }
    }

    fn transition_to_closed(&self) {
        let mut state = self.state.write();
        *state = CircuitBreakerState::Closed;
        *self.last_state_change.write() = Instant::now();
        self.failure_count.store(0, Ordering::SeqCst);
        self.success_count.store(0, Ordering::SeqCst);
        tracing::info!("Circuit breaker closed");
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

/// Retry configuration
#[derive(Clone, Debug)]
pub struct RetryConfig {
    /// Maximum number of retries
    pub max_retries: u32,
    /// Initial backoff duration
    pub initial_backoff: Duration,
    /// Maximum backoff duration
    pub max_backoff: Duration,
    /// Backoff multiplier
    pub backoff_multiplier: f64,
    /// Jitter factor (0.0 - 1.0)
    pub jitter: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            jitter: 0.1,
        }
    }
}

/// Retry policy with exponential backoff
#[derive(Clone)]
pub struct RetryPolicy {
    config: RetryConfig,
    attempt: Arc<std::sync::atomic::AtomicU64>,
}

impl RetryPolicy {
    /// Create a new retry policy
    pub fn new() -> Self {
        Self::with_config(RetryConfig::default())
    }

    /// Create a retry policy with custom config
    pub fn with_config(config: RetryConfig) -> Self {
        Self {
            config,
            attempt: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Get the next backoff duration
    pub fn next_backoff(&self) -> Option<Duration> {
        let attempt = self.attempt.load(Ordering::SeqCst);
        if attempt >= self.config.max_retries as u64 {
            return None;
        }

        // Calculate exponential backoff
        let backoff_ms = (self.config.initial_backoff.as_millis() as f64
            * self.config.backoff_multiplier.powi(attempt as i32))
        .min(self.config.max_backoff.as_millis() as f64);

        // Add jitter
        let jitter_range = backoff_ms * self.config.jitter;
        let jitter = rand::random::<f64>() * jitter_range;
        let backoff_ms = backoff_ms + jitter;

        Some(Duration::from_millis(backoff_ms as u64))
    }

    /// Increment attempt counter
    pub fn increment_attempt(&self) {
        self.attempt.fetch_add(1, Ordering::SeqCst);
    }

    /// Reset the retry policy
    pub fn reset(&self) {
        self.attempt.store(0, Ordering::SeqCst);
    }

    /// Check if more retries are allowed
    pub fn can_retry(&self) -> bool {
        self.attempt.load(Ordering::SeqCst) < self.config.max_retries as u64
    }

    /// Get current attempt number
    pub fn attempt(&self) -> u64 {
        self.attempt.load(Ordering::SeqCst)
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new()
    }
}

/// Execute a callable with retry and circuit breaker
pub async fn execute_with_retry<F, T, E, Fut>(
    circuit_breaker: Arc<CircuitBreaker>,
    retry_policy: Arc<RetryPolicy>,
    mut operation: F,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Debug + Clone + Default,
{
    loop {
        // Check circuit breaker
        if !circuit_breaker.is_available() {
            return Err(E::default());
        }

        // Execute operation
        match operation().await {
            Ok(result) => {
                circuit_breaker.record_success();
                retry_policy.reset();
                return Ok(result);
            }
            Err(e) => {
                circuit_breaker.record_failure();
                retry_policy.increment_attempt();

                // Check if we should retry
                if let Some(backoff) = retry_policy.next_backoff() {
                    tracing::warn!(
                        attempt = retry_policy.attempt(),
                        error = ?e,
                        "Retry after backoff"
                    );
                    tokio::time::sleep(backoff).await;
                    continue;
                } else {
                    return Err(e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_closed_to_open() {
        let cb = CircuitBreaker::with_config(CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        });

        assert_eq!(cb.get_state(), CircuitBreakerState::Closed);
        assert!(cb.is_available());

        cb.record_failure();
        assert!(cb.is_available());

        cb.record_failure();
        assert!(cb.is_available());

        cb.record_failure();
        assert_eq!(cb.get_state(), CircuitBreakerState::Open);
        assert!(!cb.is_available());
    }

    #[test]
    fn test_retry_policy_backoff() {
        let policy = RetryPolicy::with_config(RetryConfig {
            max_retries: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(10),
            backoff_multiplier: 2.0,
            jitter: 0.0,
        });

        assert!(policy.can_retry());
        assert_eq!(policy.attempt(), 0);

        let backoff1 = policy.next_backoff();
        assert!(backoff1.is_some());
        assert!(backoff1.unwrap() >= Duration::from_millis(100));

        policy.increment_attempt();
        let backoff2 = policy.next_backoff();
        assert!(backoff2.is_some());

        policy.increment_attempt();
        let backoff3 = policy.next_backoff();
        assert!(backoff3.is_some());

        policy.increment_attempt();
        assert!(!policy.can_retry());
        assert!(policy.next_backoff().is_none());
    }

    #[tokio::test]
    async fn test_execute_with_retry_success() {
        let cb = Arc::new(CircuitBreaker::new());
        let policy = Arc::new(RetryPolicy::new());
        let call_count = Arc::new(AtomicUsize::new(0));

        let call_count_clone = call_count.clone();
        let result = execute_with_retry(cb, policy, move || {
            let call_count_clone = call_count_clone.clone();
            async move {
                call_count_clone.fetch_add(1, Ordering::SeqCst);
                Ok::<_, String>("success")
            }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_execute_with_retry_failure_then_success() {
        let cb = Arc::new(CircuitBreaker::new());
        let policy = Arc::new(RetryPolicy::new());
        let call_count = Arc::new(AtomicUsize::new(0));

        let call_count_clone = call_count.clone();
        let result = execute_with_retry(cb, policy, move || {
            let call_count_clone = call_count_clone.clone();
            async move {
                let count = call_count_clone.fetch_add(1, Ordering::SeqCst);
                if count < 2 {
                    Err("temporary error".to_string())
                } else {
                    Ok("success")
                }
            }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }
}
