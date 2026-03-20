//! FeedbackStore implementation for PostgresStore

use super::PostgresStore;
use async_trait::async_trait;
use cogkos_core::{CogKosError, Result};
use sqlx::Row;

#[async_trait]
impl crate::FeedbackStore for PostgresStore {
    async fn insert_feedback(&self, tenant_id: &str, feedback: &cogkos_core::models::AgentFeedback) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        sqlx::query(
            r#"
            INSERT INTO agent_feedbacks (tenant_id, query_hash, agent_id, success, feedback_note, timestamp)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(tenant_id)
        .bind(feedback.query_hash as i64)
        .bind(&feedback.agent_id)
        .bind(feedback.success)
        .bind(&feedback.feedback_note)
        .bind(feedback.timestamp)
        .execute(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn get_feedback_for_query(
        &self,
        tenant_id: &str,
        query_hash: u64,
    ) -> Result<Vec<cogkos_core::models::AgentFeedback>> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let rows = sqlx::query(
            r#"
            SELECT id, query_hash, agent_id, success, feedback_note, timestamp
            FROM agent_feedbacks
            WHERE tenant_id = $1 AND query_hash = $2
            ORDER BY timestamp DESC
            "#,
        )
        .bind(tenant_id)
        .bind(query_hash as i64)
        .fetch_all(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        rows.iter()
            .map(|row| {
                Ok(cogkos_core::models::AgentFeedback {
                    query_hash: row.try_get::<i64, _>("query_hash").unwrap_or(0) as u64,
                    agent_id: row.get("agent_id"),
                    success: row.get("success"),
                    feedback_note: row.try_get("feedback_note").ok(),
                    timestamp: row.get("timestamp"),
                })
            })
            .collect()
    }
}
