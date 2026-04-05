//! CacheStore implementation for PostgresStore

use super::PostgresStore;
use async_trait::async_trait;
use cogkos_core::{CogKosError, Result};
use sqlx::Row;

#[async_trait]
impl crate::CacheStore for PostgresStore {
    async fn get_cached(
        &self,
        tenant_id: &str,
        query_hash: u64,
    ) -> Result<Option<cogkos_core::models::QueryCacheEntry>> {
        let row = sqlx::query(
            r#"
            SELECT tenant_id, query_hash, response, confidence, hit_count, success_count,
                   last_used, created_at, invalidated_by
            FROM query_cache
            WHERE tenant_id = $1 AND query_hash = $2
            "#,
        )
        .bind(tenant_id)
        .bind(query_hash as i64)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        match row {
            Some(row) => {
                let response: serde_json::Value = row
                    .try_get("response")
                    .map_err(|e| CogKosError::Database(e.to_string()))?;
                let response: cogkos_core::McpQueryResponse =
                    serde_json::from_value(response).map_err(CogKosError::Serialization)?;

                Ok(Some(cogkos_core::models::QueryCacheEntry {
                    query_hash: row.try_get::<i64, _>("query_hash").unwrap_or(0) as u64,
                    response,
                    confidence: row.try_get("confidence").unwrap_or(0.6),
                    hit_count: row.try_get::<i64, _>("hit_count").unwrap_or(0) as u64,
                    success_count: row.try_get::<i64, _>("success_count").unwrap_or(0) as u64,
                    last_used: row
                        .try_get("last_used")
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    created_at: row
                        .try_get("created_at")
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    invalidated_by: row.try_get("invalidated_by").ok(),
                }))
            }
            None => Ok(None),
        }
    }

    async fn set_cached(
        &self,
        tenant_id: &str,
        entry: &cogkos_core::models::QueryCacheEntry,
    ) -> Result<()> {
        let response_json =
            serde_json::to_value(&entry.response).map_err(CogKosError::Serialization)?;

        sqlx::query(
            r#"
            INSERT INTO query_cache (tenant_id, query_hash, response, confidence, hit_count, success_count, last_used, created_at, invalidated_by)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (tenant_id, query_hash)
            DO UPDATE SET response = $3, confidence = $4, last_used = NOW()
            "#,
        )
        .bind(tenant_id)
        .bind(entry.query_hash as i64)
        .bind(response_json)
        .bind(entry.confidence)
        .bind(entry.hit_count as i64)
        .bind(entry.success_count as i64)
        .bind(entry.last_used)
        .bind(entry.created_at)
        .bind(entry.invalidated_by)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn record_hit(&self, tenant_id: &str, query_hash: u64) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE query_cache
            SET hit_count = hit_count + 1, last_used = NOW()
            WHERE tenant_id = $1 AND query_hash = $2
            "#,
        )
        .bind(tenant_id)
        .bind(query_hash as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn record_success(&self, tenant_id: &str, query_hash: u64) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE query_cache
            SET success_count = success_count + 1
            WHERE tenant_id = $1 AND query_hash = $2
            "#,
        )
        .bind(tenant_id)
        .bind(query_hash as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn invalidate(&self, tenant_id: &str, query_hash: u64) -> Result<()> {
        sqlx::query("DELETE FROM query_cache WHERE tenant_id = $1 AND query_hash = $2")
            .bind(tenant_id)
            .bind(query_hash as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn refresh_ttl(&self, tenant_id: &str, query_hash: u64) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE query_cache
            SET last_used = NOW()
            WHERE tenant_id = $1 AND query_hash = $2
            "#,
        )
        .bind(tenant_id)
        .bind(query_hash as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }
}
