//! Vector store implementations using pgvector

use async_trait::async_trait;
use cogkos_core::models::{Id, VectorMatch};
use cogkos_core::{CogKosError, Result};
use sqlx::PgPool;
use uuid::Uuid;

/// PostgreSQL pgvector-based vector store
///
/// Vector dimension is not hardcoded — it is detected from the first
/// embedding upsert or explicitly via `ensure_index()`.
///
/// When an in-process `VectorCache` is attached via `set_cache()`,
/// read-path operations (`search`, `calculate_novelty`) are served from
/// memory, eliminating PG round-trips. Write operations write-through
/// to both PG and the cache. If the cache is unavailable, operations
/// degrade transparently to PG.
pub struct PgVectorStore {
    pool: PgPool,
    /// Detected embedding dimension (set on first upsert or explicit init)
    detected_dim: std::sync::atomic::AtomicI32,
    /// Optional in-process vector cache for fast reads.
    cache: std::sync::RwLock<Option<crate::vector_cache::VectorCache>>,
}

impl PgVectorStore {
    /// Create new pgvector store (dimension-agnostic)
    pub async fn new(pool: PgPool) -> Result<Self> {
        // Ensure pgvector extension is enabled
        sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
            .execute(&pool)
            .await
            .map_err(|e| CogKosError::Vector(e.to_string()))?;

        let store = Self {
            pool,
            detected_dim: std::sync::atomic::AtomicI32::new(0),
            cache: std::sync::RwLock::new(None),
        };

        // Try to detect dimension from existing data
        if let Ok(Some(dim)) = store.detect_dimension_from_db().await {
            store
                .detected_dim
                .store(dim, std::sync::atomic::Ordering::Relaxed);
            tracing::info!(dim, "Detected embedding dimension from existing data");
        }

        Ok(store)
    }

    /// Backwards-compatible constructor (ignores the dimension parameter)
    pub async fn with_dimension(pool: PgPool, _embedding_dimension: i32) -> Result<Self> {
        Self::new(pool).await
    }

    /// Get the detected embedding dimension (0 if not yet detected)
    pub fn embedding_dimension(&self) -> i32 {
        self.detected_dim.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Detect dimension from existing vectors in the database
    async fn detect_dimension_from_db(&self) -> Result<Option<i32>> {
        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT vector_dims(embedding)::int FROM epistemic_claims WHERE embedding IS NOT NULL LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CogKosError::Vector(e.to_string()))?;

        Ok(row.map(|r| r.0))
    }

    /// Ensure HNSW index exists for the given dimension.
    /// Called automatically on first upsert, or explicitly at startup after probing the embedding model.
    pub async fn ensure_index(&self, dim: i32) -> Result<()> {
        self.detected_dim
            .store(dim, std::sync::atomic::Ordering::Relaxed);

        // Create HNSW index if not exists (idempotent via IF NOT EXISTS)
        let sql = format!(
            "CREATE INDEX IF NOT EXISTS idx_claims_embedding_hnsw \
             ON epistemic_claims USING hnsw ((embedding::vector({})) vector_cosine_ops)",
            dim
        );
        sqlx::query(&sql)
            .execute(&self.pool)
            .await
            .map_err(|e| CogKosError::Vector(format!("HNSW index creation failed: {}", e)))?;

        tracing::info!(dim, "HNSW index ensured for embedding dimension");
        Ok(())
    }

    /// Attach an in-process vector cache for fast reads.
    pub fn set_cache(&self, cache: crate::vector_cache::VectorCache) {
        if let Ok(mut slot) = self.cache.write() {
            *slot = Some(cache);
        }
    }
}

#[async_trait]
impl super::VectorStore for PgVectorStore {
    async fn upsert(&self, id: Id, vector: Vec<f32>, metadata: serde_json::Value) -> Result<()> {
        let dim = vector.len() as i32;
        let expected = self.detected_dim.load(std::sync::atomic::Ordering::Relaxed);
        if expected == 0 {
            // First vector — set dimension and ensure HNSW index
            self.ensure_index(dim).await?;
        } else if dim != expected {
            return Err(CogKosError::Vector(format!(
                "Dimension mismatch: expected {}, got {}",
                expected, dim
            )));
        }

        // Convert vector to pgvector format: [1.0, 2.0, ...]
        let vector_str = format!(
            "[{}]",
            vector
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );

        sqlx::query("UPDATE epistemic_claims SET embedding = $2::vector WHERE id = $1")
            .bind(id)
            .bind(&vector_str)
            .execute(&self.pool)
            .await
            .map_err(|e| CogKosError::Vector(e.to_string()))?;

        // Write-through: sync cache after PG success.
        // Extract memory_layer and t_valid_end from metadata for cache filtering.
        let memory_layer = metadata
            .get("memory_layer")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let t_valid_end = metadata
            .get("t_valid_end")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        // Check if cache is present (release RwLock guard before async work).
        let has_cache = self
            .cache
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(|_| ()))
            .is_some();

        if has_cache {
            // We need tenant_id for the cache, but upsert trait doesn't pass it.
            // Query PG for tenant_id (cheap since we just wrote).
            let tenant_row: Option<(String,)> =
                sqlx::query_as("SELECT tenant_id FROM epistemic_claims WHERE id = $1")
                    .bind(id)
                    .fetch_optional(&self.pool)
                    .await
                    .ok()
                    .flatten();
            if let Some((tenant_id,)) = tenant_row {
                if let Ok(guard) = self.cache.read() {
                    if let Some(cache) = guard.as_ref() {
                        cache.upsert(id, &tenant_id, vector, memory_layer, t_valid_end);
                    }
                }
            }
        }

        Ok(())
    }

    async fn search(
        &self,
        vector: Vec<f32>,
        tenant_id: &str,
        limit: u32,
        memory_layer: Option<&str>,
    ) -> Result<Vec<VectorMatch>> {
        let expected = self.detected_dim.load(std::sync::atomic::Ordering::Relaxed);
        if expected > 0 && vector.len() as i32 != expected {
            return Err(CogKosError::Vector(format!(
                "Search dimension mismatch: expected {}, got {}",
                expected,
                vector.len()
            )));
        }

        // Try in-process cache first (no PG round-trip).
        if let Ok(guard) = self.cache.read() {
            if let Some(cache) = guard.as_ref() {
                if let Some(results) = cache.search(&vector, tenant_id, limit, memory_layer) {
                    return Ok(results);
                }
            }
        }

        // Fallback: PG query.
        let vector_str = format!(
            "[{}]",
            vector
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );

        let rows = if let Some(layer) = memory_layer {
            sqlx::query_as::<_, (Uuid, f64)>(
                "SELECT id, 1 - (embedding <=> $1::vector) as score
                 FROM epistemic_claims
                 WHERE tenant_id = $2 AND embedding IS NOT NULL
                   AND (t_valid_end IS NULL OR t_valid_end > NOW())
                   AND COALESCE(metadata->>'memory_layer', 'semantic') = $4
                 ORDER BY embedding <=> $1::vector
                 LIMIT $3",
            )
            .bind(&vector_str)
            .bind(tenant_id)
            .bind(limit as i64)
            .bind(layer)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as::<_, (Uuid, f64)>(
                "SELECT id, 1 - (embedding <=> $1::vector) as score
                 FROM epistemic_claims
                 WHERE tenant_id = $2 AND embedding IS NOT NULL
                   AND (t_valid_end IS NULL OR t_valid_end > NOW())
                 ORDER BY embedding <=> $1::vector
                 LIMIT $3",
            )
            .bind(&vector_str)
            .bind(tenant_id)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
        }
        .map_err(|e| CogKosError::Vector(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|(id, score)| VectorMatch { id, score })
            .collect())
    }

    async fn delete(&self, id: Id) -> Result<()> {
        sqlx::query("UPDATE epistemic_claims SET embedding = NULL WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| CogKosError::Vector(e.to_string()))?;

        // Write-through: remove from cache.
        if let Ok(guard) = self.cache.read() {
            if let Some(cache) = guard.as_ref() {
                cache.remove(&id);
            }
        }

        Ok(())
    }

    async fn calculate_novelty(&self, vector: Vec<f32>, tenant_id: &str) -> Result<f64> {
        // Try in-process cache first.
        if let Ok(guard) = self.cache.read() {
            if let Some(cache) = guard.as_ref() {
                if let Some(novelty) = cache.calculate_novelty(&vector, tenant_id) {
                    return Ok(novelty);
                }
            }
        }

        // Fallback: PG query.
        let vector_str = format!(
            "[{}]",
            vector
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );

        // Find nearest neighbor, novelty = distance
        let result: Option<(f64,)> = sqlx::query_as(
            "SELECT MIN(embedding <=> $1::vector) as min_dist
             FROM epistemic_claims 
             WHERE tenant_id = $2 AND embedding IS NOT NULL",
        )
        .bind(&vector_str)
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CogKosError::Vector(e.to_string()))?;

        // Cosine distance ranges [0, 2], normalize to [0, 1]
        Ok(result.map(|r| r.0 / 2.0).unwrap_or(1.0))
    }
}

/// In-memory vector store for testing
pub struct InMemoryVectorStore {
    vectors: std::sync::RwLock<std::collections::HashMap<Id, (Vec<f32>, serde_json::Value)>>,
}

impl InMemoryVectorStore {
    pub fn new() -> Self {
        Self {
            vectors: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

impl Default for InMemoryVectorStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl super::VectorStore for InMemoryVectorStore {
    async fn upsert(&self, id: Id, vector: Vec<f32>, metadata: serde_json::Value) -> Result<()> {
        self.vectors
            .write()
            .map_err(|e| CogKosError::Vector(e.to_string()))?
            .insert(id, (vector, metadata));
        Ok(())
    }

    async fn search(
        &self,
        vector: Vec<f32>,
        _tenant_id: &str,
        limit: u32,
        _memory_layer: Option<&str>,
    ) -> Result<Vec<VectorMatch>> {
        let vectors = self
            .vectors
            .read()
            .map_err(|e| CogKosError::Vector(e.to_string()))?;

        let mut scores: Vec<(Id, f64)> = vectors
            .iter()
            .map(|(id, (v, _))| (*id, cosine_similarity(&vector, v)))
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        Ok(scores
            .into_iter()
            .take(limit as usize)
            .map(|(id, score)| VectorMatch { id, score })
            .collect())
    }

    async fn delete(&self, id: Id) -> Result<()> {
        self.vectors
            .write()
            .map_err(|e| CogKosError::Vector(e.to_string()))?
            .remove(&id);
        Ok(())
    }

    async fn calculate_novelty(&self, vector: Vec<f32>, _tenant_id: &str) -> Result<f64> {
        let vectors = self
            .vectors
            .read()
            .map_err(|e| CogKosError::Vector(e.to_string()))?;

        if vectors.is_empty() {
            return Ok(1.0);
        }

        let min_similarity = vectors
            .values()
            .map(|(v, _)| cosine_similarity(&vector, v))
            .fold(0.0_f64, |a, b| a.max(b));

        Ok(1.0 - min_similarity)
    }
}

/// Calculate cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot_product: f64 = a.iter().zip(b.iter()).map(|(x, y)| (*x * *y) as f64).sum();
    let norm_a: f64 = a.iter().map(|x| (*x * *x) as f64).sum::<f64>();
    let norm_b: f64 = b.iter().map(|x| (*x * *x) as f64).sum::<f64>();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot_product / (norm_a * norm_b).sqrt()
}
