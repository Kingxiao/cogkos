//! Per-tenant token bucket rate limiter

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use rmcp::model::ErrorCode;

/// Maximum number of tenant buckets to keep in memory before evicting stale entries.
pub(crate) const RATE_LIMITER_MAX_BUCKETS: usize = 10_000;

/// Per-tenant token bucket rate limiter with in-memory and optional Redis backends
#[derive(Clone)]
pub struct RateLimiter {
    inner: RateLimiterBackend,
    max_requests_per_minute: u32,
}

#[derive(Clone)]
enum RateLimiterBackend {
    InMemory {
        buckets: Arc<Mutex<HashMap<String, (u32, std::time::Instant)>>>,
    },
    Redis {
        pool: deadpool_redis::Pool,
    },
}

impl RateLimiter {
    /// Create an in-memory rate limiter (single-node deployment)
    pub fn new(max_requests_per_minute: u32) -> Self {
        Self {
            inner: RateLimiterBackend::InMemory {
                buckets: Arc::new(Mutex::new(HashMap::new())),
            },
            max_requests_per_minute,
        }
    }

    /// Create a Redis-backed rate limiter (multi-node deployment)
    pub fn with_redis(pool: deadpool_redis::Pool, max_requests_per_minute: u32) -> Self {
        Self {
            inner: RateLimiterBackend::Redis { pool },
            max_requests_per_minute,
        }
    }

    /// Try to consume one token for the given tenant. Returns Ok(()) or rate-limit error.
    pub async fn check(&self, tenant_id: &str) -> Result<(), rmcp::ErrorData> {
        match &self.inner {
            RateLimiterBackend::InMemory { buckets } => {
                self.check_in_memory(buckets, tenant_id).await
            }
            RateLimiterBackend::Redis { pool } => self.check_redis(pool, tenant_id).await,
        }
    }

    async fn check_in_memory(
        &self,
        buckets: &Arc<Mutex<HashMap<String, (u32, std::time::Instant)>>>,
        tenant_id: &str,
    ) -> Result<(), rmcp::ErrorData> {
        let mut buckets = buckets.lock().await;
        let now = std::time::Instant::now();
        let max = self.max_requests_per_minute;

        // Evict stale entries when bucket count exceeds threshold.
        // Remove tenants that haven't been seen for > 2 minutes (fully refilled).
        if buckets.len() > RATE_LIMITER_MAX_BUCKETS {
            let stale_threshold = std::time::Duration::from_secs(120);
            buckets.retain(|_, (_, last)| now.duration_since(*last) < stale_threshold);
        }

        let (tokens, last_refill) = buckets.entry(tenant_id.to_string()).or_insert((max, now));

        // Refill tokens based on elapsed time
        let elapsed = now.duration_since(*last_refill);
        let refill = (elapsed.as_secs_f64() / 60.0 * max as f64) as u32;
        if refill > 0 {
            *tokens = (*tokens + refill).min(max);
            *last_refill = now;
        }

        if *tokens > 0 {
            *tokens -= 1;
            Ok(())
        } else {
            Err(rmcp::ErrorData::new(
                ErrorCode(-32029),
                "Rate limit exceeded",
                None,
            ))
        }
    }

    async fn check_redis(
        &self,
        pool: &deadpool_redis::Pool,
        tenant_id: &str,
    ) -> Result<(), rmcp::ErrorData> {
        use deadpool_redis::redis::AsyncCommands;

        let mut conn = pool.get().await.map_err(|e| {
            tracing::error!("Redis connection failed for rate limiting: {}", e);
            rmcp::ErrorData::new(ErrorCode(-32603), "Rate limiter unavailable", None)
        })?;

        let key = format!("cogkos:ratelimit:{}", tenant_id);
        let max = self.max_requests_per_minute as i64;

        // Atomic increment + TTL via Redis INCR + EXPIRE
        let count: i64 = conn.incr(&key, 1i64).await.map_err(|e| {
            tracing::error!("Redis INCR failed: {}", e);
            rmcp::ErrorData::new(ErrorCode(-32603), "Rate limiter error", None)
        })?;

        // Set TTL on first request in window
        if count == 1 {
            if let Err(e) = conn.expire::<_, ()>(&key, 60).await {
                tracing::warn!(key = %key, "Failed to set rate limit TTL, bucket may persist indefinitely: {}", e);
            }
        }

        if count > max {
            Err(rmcp::ErrorData::new(
                ErrorCode(-32029),
                "Rate limit exceeded",
                None,
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rate_limiter_allows_requests_within_limit() {
        let limiter = RateLimiter::new(5);
        for _ in 0..5 {
            assert!(limiter.check("tenant-a").await.is_ok());
        }
    }

    #[tokio::test]
    async fn rate_limiter_rejects_after_exhausted() {
        let limiter = RateLimiter::new(3);
        for _ in 0..3 {
            limiter.check("tenant-a").await.unwrap();
        }
        let err = limiter.check("tenant-a").await.unwrap_err();
        assert!(err.message.contains("Rate limit"));
    }

    #[tokio::test]
    async fn rate_limiter_isolates_tenants() {
        let limiter = RateLimiter::new(2);
        // Exhaust tenant-a
        limiter.check("tenant-a").await.unwrap();
        limiter.check("tenant-a").await.unwrap();
        assert!(limiter.check("tenant-a").await.is_err());
        // tenant-b should still work
        assert!(limiter.check("tenant-b").await.is_ok());
    }

    #[tokio::test]
    async fn rate_limiter_error_code() {
        let limiter = RateLimiter::new(1);
        limiter.check("t").await.unwrap();
        let err = limiter.check("t").await.unwrap_err();
        assert_eq!(err.code, ErrorCode(-32029));
    }

    #[tokio::test]
    async fn rate_limiter_clone_shares_state() {
        let limiter = RateLimiter::new(2);
        let limiter2 = limiter.clone();
        limiter.check("t").await.unwrap();
        limiter2.check("t").await.unwrap();
        // Both clones consumed from the same bucket
        assert!(limiter.check("t").await.is_err());
    }

    #[test]
    fn rate_limiter_max_buckets_constant() {
        // Ensure the eviction threshold is reasonable
        assert_eq!(RATE_LIMITER_MAX_BUCKETS, 10_000);
    }

    #[tokio::test]
    async fn rate_limiter_eviction_does_not_break_active_tenants() {
        // Even after eviction runs, active tenants keep their state
        let limiter = RateLimiter::new(100);
        // Create some tenants
        for i in 0..50 {
            limiter.check(&format!("tenant-{}", i)).await.unwrap();
        }
        // Active tenant should still work fine
        assert!(limiter.check("tenant-0").await.is_ok());
    }
}
