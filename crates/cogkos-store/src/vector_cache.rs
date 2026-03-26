//! In-process vector cache for fast read-path queries.
//!
//! Eliminates PG connection pool as the read bottleneck for vector search.
//! Falls back to PG when the cache is cold or disabled.

use cogkos_core::models::{Id, VectorMatch};
use std::collections::HashMap;
use std::sync::RwLock;
use tracing::{info, warn};

/// Maximum memory budget (bytes) before auto-disabling the cache.
const MAX_MEMORY_BYTES: usize = 1_024 * 1_024 * 1_024; // 1 GB

/// Maximum warm-up duration before auto-disabling the cache.
const MAX_WARMUP_SECS: u64 = 30;

struct CachedVector {
    tenant_id: String,
    embedding: Vec<f32>,
    memory_layer: Option<String>,
    t_valid_end: Option<chrono::DateTime<chrono::Utc>>,
}

/// In-process vector cache backed by a simple HashMap.
///
/// - Startup: `warm_from_pg` loads all embeddings into memory.
/// - Read path: `search` does brute-force cosine similarity (pure Rust).
/// - Write path: caller upserts PG first, then calls `upsert` to sync the cache.
/// - Degradation: if the cache is disabled or poisoned, callers fall back to PG.
pub struct VectorCache {
    vectors: RwLock<HashMap<Id, CachedVector>>,
    enabled: bool,
}

impl VectorCache {
    /// Create an empty, enabled cache.
    pub fn new() -> Self {
        Self {
            vectors: RwLock::new(HashMap::new()),
            enabled: true,
        }
    }

    /// Create a disabled (no-op) cache.
    pub fn disabled() -> Self {
        Self {
            vectors: RwLock::new(HashMap::new()),
            enabled: false,
        }
    }

    /// Whether the cache is enabled and usable.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Number of vectors currently cached.
    pub fn len(&self) -> usize {
        self.vectors.read().map(|v| v.len()).unwrap_or(0)
    }

    /// Load all embeddings from PostgreSQL into memory.
    ///
    /// Auto-disables if:
    /// - estimated memory exceeds `MAX_MEMORY_BYTES` (1 GB)
    /// - loading takes longer than `MAX_WARMUP_SECS` (30 s)
    pub async fn warm_from_pg(pool: &sqlx::PgPool) -> Self {
        let start = std::time::Instant::now();

        // First, estimate the data size to avoid loading too much.
        let count_row: Option<(i64,)> =
            sqlx::query_as("SELECT COUNT(*) FROM epistemic_claims WHERE embedding IS NOT NULL")
                .fetch_optional(pool)
                .await
                .ok()
                .flatten();

        let count = count_row.map(|r| r.0 as usize).unwrap_or(0);
        if count == 0 {
            info!("Vector cache: no embeddings in PG, starting empty");
            return Self::new();
        }

        // Detect dimension from first row.
        let dim_row: Option<(i32,)> = sqlx::query_as(
            "SELECT vector_dims(embedding)::int FROM epistemic_claims \
             WHERE embedding IS NOT NULL LIMIT 1",
        )
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

        let dim = dim_row.map(|r| r.0 as usize).unwrap_or(0);
        if dim == 0 {
            warn!("Vector cache: could not detect embedding dimension, disabling");
            return Self::disabled();
        }

        // Memory estimate: count * dim * 4 bytes + overhead (~128 bytes/entry).
        let estimated_bytes = count * (dim * 4 + 128);
        if estimated_bytes > MAX_MEMORY_BYTES {
            warn!(
                count,
                dim,
                estimated_mb = estimated_bytes / (1024 * 1024),
                "Vector cache: estimated memory exceeds 1 GB, disabling"
            );
            return Self::disabled();
        }

        // Stream rows in batches.
        let mut map: HashMap<Id, CachedVector> = HashMap::with_capacity(count);

        // pgvector stores as vector type; cast to text for parsing.
        let rows = sqlx::query_as::<
            _,
            (
                uuid::Uuid,
                String,
                String,
                Option<String>,
                Option<chrono::DateTime<chrono::Utc>>,
            ),
        >(
            "SELECT id, tenant_id, embedding::text, \
                    COALESCE(metadata->>'memory_layer', 'semantic'), \
                    t_valid_end \
             FROM epistemic_claims WHERE embedding IS NOT NULL",
        )
        .fetch_all(pool)
        .await;

        let rows = match rows {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "Vector cache: failed to load from PG, disabling");
                return Self::disabled();
            }
        };

        if start.elapsed().as_secs() > MAX_WARMUP_SECS {
            warn!(
                elapsed_secs = start.elapsed().as_secs(),
                "Vector cache: warm-up exceeded 30s, disabling"
            );
            return Self::disabled();
        }

        for (id, tenant_id, embedding_text, memory_layer, t_valid_end) in rows {
            if let Some(embedding) = parse_pg_vector(&embedding_text) {
                map.insert(
                    id,
                    CachedVector {
                        tenant_id,
                        embedding,
                        memory_layer,
                        t_valid_end,
                    },
                );
            }
        }

        let elapsed = start.elapsed();
        if elapsed.as_secs() > MAX_WARMUP_SECS {
            warn!(
                elapsed_secs = elapsed.as_secs(),
                "Vector cache: warm-up exceeded 30s after parsing, disabling"
            );
            return Self::disabled();
        }

        info!(
            vectors = map.len(),
            dim,
            elapsed_ms = elapsed.as_millis() as u64,
            estimated_mb = (map.len() * (dim * 4 + 128)) / (1024 * 1024),
            "Vector cache warmed from PostgreSQL"
        );

        Self {
            vectors: RwLock::new(map),
            enabled: true,
        }
    }

    /// Brute-force cosine similarity search (pure Rust).
    ///
    /// Filters by tenant, memory_layer, and time validity.
    pub fn search(
        &self,
        query_vec: &[f32],
        tenant_id: &str,
        limit: u32,
        memory_layer: Option<&str>,
    ) -> Option<Vec<VectorMatch>> {
        if !self.enabled {
            return None;
        }

        let vectors = match self.vectors.read() {
            Ok(v) => v,
            Err(_) => return None, // poisoned lock -> fall back to PG
        };

        if vectors.is_empty() {
            return None; // cold cache -> fall back to PG
        }

        let now = chrono::Utc::now();

        let mut scores: Vec<(Id, f64)> = vectors
            .iter()
            .filter(|(_, v)| v.tenant_id == tenant_id)
            .filter(|(_, v)| match memory_layer {
                Some(layer) => v.memory_layer.as_deref().unwrap_or("semantic") == layer,
                None => true,
            })
            .filter(|(_, v)| v.t_valid_end.map_or(true, |end| end > now))
            .map(|(id, v)| (*id, cosine_similarity(query_vec, &v.embedding)))
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        Some(
            scores
                .into_iter()
                .take(limit as usize)
                .map(|(id, score)| VectorMatch { id, score })
                .collect(),
        )
    }

    /// Calculate novelty (inverse of max similarity) from the cache.
    pub fn calculate_novelty(&self, query_vec: &[f32], tenant_id: &str) -> Option<f64> {
        if !self.enabled {
            return None;
        }

        let vectors = match self.vectors.read() {
            Ok(v) => v,
            Err(_) => return None,
        };

        if vectors.is_empty() {
            return None;
        }

        let max_sim = vectors
            .iter()
            .filter(|(_, v)| v.tenant_id == tenant_id)
            .map(|(_, v)| cosine_similarity(query_vec, &v.embedding))
            .fold(0.0_f64, f64::max);

        // Cosine similarity [0,1] -> novelty [0,1]
        Some(1.0 - max_sim)
    }

    /// Insert or update a cached vector (call after PG upsert succeeds).
    pub fn upsert(
        &self,
        id: Id,
        tenant_id: &str,
        embedding: Vec<f32>,
        memory_layer: Option<String>,
        t_valid_end: Option<chrono::DateTime<chrono::Utc>>,
    ) {
        if !self.enabled {
            return;
        }
        if let Ok(mut vectors) = self.vectors.write() {
            vectors.insert(
                id,
                CachedVector {
                    tenant_id: tenant_id.to_string(),
                    embedding,
                    memory_layer,
                    t_valid_end,
                },
            );
        }
    }

    /// Remove a vector from the cache.
    pub fn remove(&self, id: &Id) {
        if !self.enabled {
            return;
        }
        if let Ok(mut vectors) = self.vectors.write() {
            vectors.remove(id);
        }
    }
}

/// Parse pgvector text representation "[1.0,2.0,3.0]" into Vec<f32>.
fn parse_pg_vector(text: &str) -> Option<Vec<f32>> {
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    if inner.is_empty() {
        return Some(Vec::new());
    }
    inner
        .split(',')
        .map(|s| s.trim().parse::<f32>().ok())
        .collect()
}

/// Pure Rust cosine similarity.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;

    for (x, y) in a.iter().zip(b.iter()) {
        let fx = *x as f64;
        let fy = *y as f64;
        dot += fx * fy;
        norm_a += fx * fx;
        norm_b += fy * fy;
    }

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a * norm_b).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pg_vector() {
        let v = parse_pg_vector("[1.0,2.0,3.0]").unwrap();
        assert_eq!(v, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_parse_pg_vector_spaces() {
        let v = parse_pg_vector("[1.0, 2.0, 3.0]").unwrap();
        assert_eq!(v, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_parse_pg_vector_empty() {
        let v = parse_pg_vector("[]").unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn test_parse_pg_vector_invalid() {
        assert!(parse_pg_vector("not a vector").is_none());
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let a = vec![1.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_search_filters_tenant() {
        let cache = VectorCache::new();
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        cache.upsert(id1, "tenant-a", vec![1.0, 0.0, 0.0], None, None);
        cache.upsert(id2, "tenant-b", vec![1.0, 0.0, 0.0], None, None);

        let results = cache
            .search(&[1.0, 0.0, 0.0], "tenant-a", 10, None)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id1);
    }

    #[test]
    fn test_search_filters_memory_layer() {
        let cache = VectorCache::new();
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        cache.upsert(
            id1,
            "t1",
            vec![1.0, 0.0],
            Some("semantic".to_string()),
            None,
        );
        cache.upsert(id2, "t1", vec![1.0, 0.0], Some("working".to_string()), None);

        let results = cache
            .search(&[1.0, 0.0], "t1", 10, Some("working"))
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id2);
    }

    #[test]
    fn test_search_filters_expired() {
        let cache = VectorCache::new();
        let id1 = uuid::Uuid::new_v4();
        let past = chrono::Utc::now() - chrono::Duration::hours(1);
        cache.upsert(id1, "t1", vec![1.0, 0.0], None, Some(past));

        let results = cache.search(&[1.0, 0.0], "t1", 10, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_remove() {
        let cache = VectorCache::new();
        let id = uuid::Uuid::new_v4();
        cache.upsert(id, "t1", vec![1.0], None, None);
        assert_eq!(cache.len(), 1);
        cache.remove(&id);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_disabled_cache_returns_none() {
        let cache = VectorCache::disabled();
        assert!(cache.search(&[1.0], "t", 10, None).is_none());
        assert!(cache.calculate_novelty(&[1.0], "t").is_none());
    }
}
