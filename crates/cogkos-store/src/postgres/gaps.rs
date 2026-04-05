//! GapStore implementation for PostgresStore

use super::PostgresStore;
use crate::KnowledgeGapRecord;
use async_trait::async_trait;
use cogkos_core::{CogKosError, Result};
use sqlx::Row;

#[async_trait]
impl crate::GapStore for PostgresStore {
    async fn record_gap(&self, gap: &KnowledgeGapRecord) -> Result<uuid::Uuid> {
        let gap_id = gap.gap_id;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), &gap.tenant_id).await?;

        sqlx::query(
            r#"
            INSERT INTO knowledge_gaps (gap_id, tenant_id, domain, description, priority, status, reported_at, filled_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (tenant_id, domain, description)
            DO UPDATE SET status = 'open', reported_at = NOW()
            RETURNING gap_id
            "#,
        )
        .bind(gap.gap_id)
        .bind(&gap.tenant_id)
        .bind(&gap.domain)
        .bind(&gap.description)
        .bind(&gap.priority)
        .bind(&gap.status)
        .bind(gap.reported_at)
        .bind(gap.filled_at)
        .fetch_one(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(gap_id)
    }

    async fn find_similar_gap(
        &self,
        tenant_id: &str,
        domain: &str,
        description: &str,
    ) -> Result<Option<KnowledgeGapRecord>> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let result = sqlx::query(
            r#"
            SELECT gap_id, tenant_id, domain, description, priority, status, reported_at, filled_at
            FROM knowledge_gaps
            WHERE tenant_id = $1 AND domain = $2 AND description = $3 AND status = 'open'
            "#,
        )
        .bind(tenant_id)
        .bind(domain)
        .bind(description)
        .fetch_optional(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(result.map(|row| KnowledgeGapRecord {
            gap_id: row.get("gap_id"),
            tenant_id: row.get("tenant_id"),
            domain: row.get("domain"),
            description: row.get("description"),
            priority: row.get("priority"),
            status: row.get("status"),
            reported_at: row.get("reported_at"),
            filled_at: row.get("filled_at"),
        }))
    }

    async fn get_gaps(&self, tenant_id: &str) -> Result<Vec<KnowledgeGapRecord>> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let results = sqlx::query(
            r#"
            SELECT gap_id, tenant_id, domain, description, priority, status, reported_at, filled_at
            FROM knowledge_gaps
            WHERE tenant_id = $1
            ORDER BY reported_at DESC
            "#,
        )
        .bind(tenant_id)
        .fetch_all(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(results
            .into_iter()
            .map(|row| KnowledgeGapRecord {
                gap_id: row.get("gap_id"),
                tenant_id: row.get("tenant_id"),
                domain: row.get("domain"),
                description: row.get("description"),
                priority: row.get("priority"),
                status: row.get("status"),
                reported_at: row.get("reported_at"),
                filled_at: row.get("filled_at"),
            })
            .collect())
    }

    async fn get_gaps_by_domain(
        &self,
        tenant_id: &str,
        domain: &str,
    ) -> Result<Vec<KnowledgeGapRecord>> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let results = sqlx::query(
            r#"
            SELECT gap_id, tenant_id, domain, description, priority, status, reported_at, filled_at
            FROM knowledge_gaps
            WHERE tenant_id = $1 AND domain = $2
            ORDER BY reported_at DESC
            "#,
        )
        .bind(tenant_id)
        .bind(domain)
        .fetch_all(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(results
            .into_iter()
            .map(|row| KnowledgeGapRecord {
                gap_id: row.get("gap_id"),
                tenant_id: row.get("tenant_id"),
                domain: row.get("domain"),
                description: row.get("description"),
                priority: row.get("priority"),
                status: row.get("status"),
                reported_at: row.get("reported_at"),
                filled_at: row.get("filled_at"),
            })
            .collect())
    }

    async fn mark_gap_filled(&self, gap_id: uuid::Uuid, tenant_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE knowledge_gaps
            SET status = 'filled', filled_at = NOW()
            WHERE gap_id = $1 AND tenant_id = $2
            "#,
        )
        .bind(gap_id)
        .bind(tenant_id)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }
}

/// PostgreSQL gap record helper
#[allow(dead_code)]
struct PgGapRecord {
    gap_id: uuid::Uuid,
    tenant_id: String,
    domain: String,
    description: String,
    priority: String,
    status: String,
    reported_at: chrono::DateTime<chrono::Utc>,
    filled_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<PgGapRecord> for KnowledgeGapRecord {
    fn from(r: PgGapRecord) -> Self {
        Self {
            gap_id: r.gap_id,
            tenant_id: r.tenant_id,
            domain: r.domain,
            description: r.description,
            priority: r.priority,
            status: r.status,
            reported_at: r.reported_at,
            filled_at: r.filled_at,
        }
    }
}
