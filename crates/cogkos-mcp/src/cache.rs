use cogkos_core::Result;
use cogkos_core::models::*;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::RwLock;

/// In-memory query cache
pub struct QueryCache {
    cache: Arc<RwLock<LruCache<u64, QueryCacheEntry>>>,
    ttl_seconds: i64,
}

impl QueryCache {
    /// Create new query cache
    pub fn new(max_entries: usize, ttl_seconds: i64) -> Self {
        let capacity = NonZeroUsize::new(max_entries.max(1)).unwrap();
        Self {
            cache: Arc::new(RwLock::new(LruCache::new(capacity))),
            ttl_seconds,
        }
    }

    /// Get cached entry
    pub async fn get(&self, query_hash: u64) -> Result<Option<QueryCacheEntry>> {
        let mut cache = self.cache.write().await;

        if let Some(entry) = cache.get(&query_hash) {
            if entry.is_valid(self.ttl_seconds) {
                return Ok(Some(entry.clone()));
            } else {
                // Remove expired entry
                cache.pop(&query_hash);
            }
        }

        Ok(None)
    }

    /// Set cached entry
    pub async fn set(&self, entry: QueryCacheEntry) -> Result<()> {
        let mut cache = self.cache.write().await;
        cache.put(entry.query_hash, entry);
        Ok(())
    }

    /// Record cache hit
    pub async fn record_hit(&self, query_hash: u64) -> Result<()> {
        let mut cache = self.cache.write().await;
        if let Some(entry) = cache.get_mut(&query_hash) {
            entry.record_hit();
        }
        Ok(())
    }

    /// Record success feedback
    pub async fn record_success(&self, query_hash: u64) -> Result<()> {
        let mut cache = self.cache.write().await;
        if let Some(entry) = cache.get_mut(&query_hash) {
            entry.record_success();
        }
        Ok(())
    }

    /// Invalidate entries related to a claim
    pub async fn invalidate_by_claim(&self, claim_id: uuid::Uuid) -> Result<u32> {
        let mut cache = self.cache.write().await;
        let mut to_remove = Vec::new();

        for (hash, entry) in cache.iter() {
            // Check if response contains the claim
            if let Some(ref belief) = entry.response.best_belief
                && belief.claim_ids.contains(&claim_id)
            {
                to_remove.push(*hash);
            }
        }

        let count = to_remove.len() as u32;
        for hash in to_remove {
            cache.pop(&hash);
        }

        Ok(count)
    }

    /// Get cache stats
    pub async fn stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        CacheStats {
            entries: cache.len(),
            capacity: cache.cap().get(),
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub entries: usize,
    pub capacity: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use cogkos_core::models::{
        CacheStatus, FreshnessInfo, McpQueryResponse, QueryCacheEntry, QueryMetadata,
    };

    fn make_response(query_hash: u64) -> McpQueryResponse {
        McpQueryResponse {
            query_hash,
            query_context: format!("test query {}", query_hash),
            best_belief: None,
            related_by_graph: vec![],
            conflicts: vec![],
            prediction: None,
            knowledge_gaps: vec![],
            freshness: FreshnessInfo::default(),
            cache_status: CacheStatus::Miss,
            cognitive_path: None,
            metadata: QueryMetadata::default(),
        }
    }

    fn make_entry(query_hash: u64) -> QueryCacheEntry {
        QueryCacheEntry::new(query_hash, make_response(query_hash))
    }

    #[tokio::test]
    async fn query_cache_new_creates_empty() {
        let cache = QueryCache::new(10, 3600);
        let stats = cache.stats().await;
        assert_eq!(stats.entries, 0);
    }

    #[tokio::test]
    async fn query_cache_set_and_get() {
        let cache = QueryCache::new(10, 3600);
        let entry = make_entry(42);
        cache.set(entry).await.unwrap();

        let result = cache.get(42).await.unwrap();
        assert!(result.is_some());
        let got = result.unwrap();
        assert_eq!(got.query_hash, 42);
        assert_eq!(got.response.query_context, "test query 42");
    }

    #[tokio::test]
    async fn query_cache_get_expired_returns_none() {
        // TTL of 0 seconds means entries expire immediately
        let cache = QueryCache::new(10, 0);
        let entry = make_entry(99);
        cache.set(entry).await.unwrap();

        // Entry should be expired (ttl=0, is_valid checks age >= ttl_seconds)
        let result = cache.get(99).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn query_cache_record_hit() {
        let cache = QueryCache::new(10, 3600);
        let entry = make_entry(1);
        cache.set(entry).await.unwrap();
        // Should not panic
        cache.record_hit(1).await.unwrap();
        // Also should not panic for nonexistent
        cache.record_hit(999).await.unwrap();
    }

    #[tokio::test]
    async fn query_cache_record_success() {
        let cache = QueryCache::new(10, 3600);
        let entry = make_entry(1);
        cache.set(entry).await.unwrap();
        // Should not panic
        cache.record_success(1).await.unwrap();
        // Also should not panic for nonexistent
        cache.record_success(999).await.unwrap();
    }

    #[tokio::test]
    async fn query_cache_invalidate_by_claim() {
        use cogkos_core::models::{BeliefSummary, ConsolidationStage};

        let cache = QueryCache::new(10, 3600);
        let claim_id = uuid::Uuid::new_v4();

        // Create entry with a best_belief referencing the claim
        let mut response = make_response(10);
        response.best_belief = Some(BeliefSummary {
            claim_id: Some(claim_id),
            content: "test belief".to_string(),
            confidence: 0.9,
            based_on: 1,
            consolidation_stage: ConsolidationStage::FastTrack,
            claim_ids: vec![claim_id],
        });
        let entry = QueryCacheEntry::new(10, response);
        cache.set(entry).await.unwrap();

        // Also add one without the claim
        cache.set(make_entry(20)).await.unwrap();

        let removed = cache.invalidate_by_claim(claim_id).await.unwrap();
        assert_eq!(removed, 1);

        // The matching entry should be gone
        assert!(cache.get(10).await.unwrap().is_none());
        // The other entry should remain
        assert!(cache.get(20).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn query_cache_stats_shows_correct_count() {
        let cache = QueryCache::new(10, 3600);
        cache.set(make_entry(1)).await.unwrap();
        cache.set(make_entry(2)).await.unwrap();
        cache.set(make_entry(3)).await.unwrap();

        let stats = cache.stats().await;
        assert_eq!(stats.entries, 3);
        assert_eq!(stats.capacity, 10);
    }

    #[tokio::test]
    async fn query_cache_capacity_limit() {
        let cache = QueryCache::new(2, 3600);
        cache.set(make_entry(1)).await.unwrap();
        cache.set(make_entry(2)).await.unwrap();
        cache.set(make_entry(3)).await.unwrap();

        let stats = cache.stats().await;
        assert_eq!(stats.entries, 2);
        // The oldest (LRU) entry should have been evicted
        assert!(cache.get(1).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn query_cache_get_nonexistent_returns_none() {
        let cache = QueryCache::new(10, 3600);
        let result = cache.get(12345).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn query_cache_invalidate_nonexistent_returns_zero() {
        let cache = QueryCache::new(10, 3600);
        let count = cache
            .invalidate_by_claim(uuid::Uuid::new_v4())
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
