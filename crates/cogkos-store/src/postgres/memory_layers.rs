//! MemoryLayerStore implementation for PostgresStore

use super::PostgresStore;
use super::claims::row_to_claim;
use async_trait::async_trait;
use cogkos_core::{CogKosError, Result};
use cogkos_core::models::EpistemicClaim;

#[async_trait]
impl crate::MemoryLayerStore for PostgresStore {
    async fn list_claims_by_memory_layer(
        &self,
        tenant_id: &str,
        memory_layer: &str,
        session_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EpistemicClaim>> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let rows = if let Some(sid) = session_id {
            sqlx::query(
                r#"
                SELECT id, tenant_id, content, node_type, epistemic_status, confidence,
                    consolidation_stage, claimant, provenance, access_envelope,
                    activation_weight, access_count, last_accessed, t_valid_start, t_valid_end,
                    t_known, vector_id, last_prediction_error, derived_from, needs_revalidation,
                    durability, created_at, updated_at, metadata
                FROM epistemic_claims
                WHERE tenant_id = $1
                  AND metadata->>'memory_layer' = $2
                  AND metadata->>'session_id' = $3
                ORDER BY activation_weight DESC
                LIMIT $4
                "#,
            )
            .bind(tenant_id)
            .bind(memory_layer)
            .bind(sid)
            .bind(limit as i64)
            .fetch_all(tx.as_mut())
            .await
        } else {
            sqlx::query(
                r#"
                SELECT id, tenant_id, content, node_type, epistemic_status, confidence,
                    consolidation_stage, claimant, provenance, access_envelope,
                    activation_weight, access_count, last_accessed, t_valid_start, t_valid_end,
                    t_known, vector_id, last_prediction_error, derived_from, needs_revalidation,
                    durability, created_at, updated_at, metadata
                FROM epistemic_claims
                WHERE tenant_id = $1
                  AND metadata->>'memory_layer' = $2
                ORDER BY activation_weight DESC
                LIMIT $3
                "#,
            )
            .bind(tenant_id)
            .bind(memory_layer)
            .bind(limit as i64)
            .fetch_all(tx.as_mut())
            .await
        }
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        rows.iter().map(row_to_claim).collect()
    }

    async fn count_claims_by_memory_layer(
        &self,
        tenant_id: &str,
        memory_layer: &str,
        session_id: Option<&str>,
    ) -> Result<usize> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let count: i64 = if let Some(sid) = session_id {
            sqlx::query_scalar(
                r#"
                SELECT COUNT(*) FROM epistemic_claims
                WHERE tenant_id = $1
                  AND metadata->>'memory_layer' = $2
                  AND metadata->>'session_id' = $3
                "#,
            )
            .bind(tenant_id)
            .bind(memory_layer)
            .bind(sid)
            .fetch_one(tx.as_mut())
            .await
        } else {
            sqlx::query_scalar(
                r#"
                SELECT COUNT(*) FROM epistemic_claims
                WHERE tenant_id = $1
                  AND metadata->>'memory_layer' = $2
                "#,
            )
            .bind(tenant_id)
            .bind(memory_layer)
            .fetch_one(tx.as_mut())
            .await
        }
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(count as usize)
    }

    async fn gc_expired_memory_layer(
        &self,
        tenant_id: &str,
        memory_layer: &str,
        max_age_hours: f64,
    ) -> Result<usize> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let result = sqlx::query(
            r#"
            DELETE FROM epistemic_claims
            WHERE tenant_id = $1
              AND metadata->>'memory_layer' = $2
              AND created_at < NOW() - ($3 || ' hours')::interval
            "#,
        )
        .bind(tenant_id)
        .bind(memory_layer)
        .bind(max_age_hours.to_string())
        .execute(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(result.rows_affected() as usize)
    }

    async fn promote_memory_layer(
        &self,
        tenant_id: &str,
        from_layer: &str,
        to_layer: &str,
        min_rehearsal_count: u64,
    ) -> Result<usize> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let result = sqlx::query(
            r#"
            UPDATE epistemic_claims
            SET metadata = jsonb_set(
                    jsonb_set(metadata, '{memory_layer}', to_jsonb($3::text)),
                    '{rehearsal_count}', '0'::jsonb
                ),
                updated_at = NOW()
            WHERE tenant_id = $1
              AND metadata->>'memory_layer' = $2
              AND COALESCE((metadata->>'rehearsal_count')::bigint, 0) >= $4
            "#,
        )
        .bind(tenant_id)
        .bind(from_layer)
        .bind(to_layer)
        .bind(min_rehearsal_count as i64)
        .execute(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(result.rows_affected() as usize)
    }
}
