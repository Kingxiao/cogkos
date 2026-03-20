//! SubscriptionStore implementation for PostgresStore

use super::PostgresStore;
use async_trait::async_trait;
use cogkos_core::{CogKosError, Result};
use sqlx::postgres::PgRow;

#[async_trait]
impl crate::SubscriptionStore for PostgresStore {
    async fn create_subscription(
        &self,
        subscription: &cogkos_core::models::SubscriptionSource,
    ) -> Result<uuid::Uuid> {
        let config_json =
            serde_json::to_value(&subscription.config).map_err(CogKosError::Serialization)?;
        let claimant_json = serde_json::to_value(&subscription.claimant_template)
            .map_err(CogKosError::Serialization)?;

        sqlx::query(
            r#"
            INSERT INTO subscriptions (
                id, tenant_id, name, source_type, config, poll_interval_secs,
                claimant_template, base_confidence, enabled, last_polled, error_count,
                created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NOW(), NOW())
        "#,
        )
        .bind(subscription.id)
        .bind(&subscription.tenant_id)
        .bind(&subscription.name)
        .bind(subscription.source_type.as_db_str())
        .bind(config_json)
        .bind(subscription.poll_interval_secs as i64)
        .bind(claimant_json)
        .bind(subscription.base_confidence)
        .bind(subscription.enabled)
        .bind(subscription.last_polled)
        .bind(subscription.error_count as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(subscription.id)
    }

    async fn get_subscription(
        &self,
        tenant_id: &str,
        id: uuid::Uuid,
    ) -> Result<cogkos_core::models::SubscriptionSource> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, name, source_type, config, poll_interval_secs,
                   claimant_template, base_confidence, enabled, last_polled, error_count,
                   created_at, updated_at
            FROM subscriptions
            WHERE id = $1 AND tenant_id = $2
            "#,
        )
        .bind(id)
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        match row {
            Some(row) => row_to_subscription(&row),
            None => Err(CogKosError::NotFound(format!(
                "Subscription {} not found",
                id
            ))),
        }
    }

    async fn update_subscription(
        &self,
        subscription: &cogkos_core::models::SubscriptionSource,
    ) -> Result<()> {
        let config_json =
            serde_json::to_value(&subscription.config).map_err(CogKosError::Serialization)?;
        let claimant_json = serde_json::to_value(&subscription.claimant_template)
            .map_err(CogKosError::Serialization)?;

        sqlx::query(
            r#"
            UPDATE subscriptions SET
                name = $1,
                source_type = $2,
                config = $3,
                poll_interval_secs = $4,
                claimant_template = $5,
                base_confidence = $6,
                enabled = $7,
                last_polled = $8,
                error_count = $9,
                updated_at = NOW()
            WHERE id = $10 AND tenant_id = $11
        "#,
        )
        .bind(&subscription.name)
        .bind(subscription.source_type.as_db_str())
        .bind(config_json)
        .bind(subscription.poll_interval_secs as i64)
        .bind(claimant_json)
        .bind(subscription.base_confidence)
        .bind(subscription.enabled)
        .bind(subscription.last_polled)
        .bind(subscription.error_count as i64)
        .bind(subscription.id)
        .bind(&subscription.tenant_id)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn delete_subscription(&self, tenant_id: &str, id: uuid::Uuid) -> Result<()> {
        sqlx::query("DELETE FROM subscriptions WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(tenant_id)
            .execute(&self.pool)
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn list_subscriptions(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<cogkos_core::models::SubscriptionSource>> {
        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, name, source_type, config, poll_interval_secs,
                   claimant_template, base_confidence, enabled, last_polled, error_count,
                   created_at, updated_at
            FROM subscriptions
            WHERE tenant_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        rows.iter().map(row_to_subscription).collect()
    }

    async fn list_enabled_subscriptions(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<cogkos_core::models::SubscriptionSource>> {
        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, name, source_type, config, poll_interval_secs,
                   claimant_template, base_confidence, enabled, last_polled, error_count,
                   created_at, updated_at
            FROM subscriptions
            WHERE tenant_id = $1 AND enabled = true
            ORDER BY created_at DESC
            "#,
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        rows.iter().map(row_to_subscription).collect()
    }

    async fn update_subscription_status(&self, id: uuid::Uuid, tenant_id: &str, _status: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE subscriptions
            SET last_polled = NOW(), updated_at = NOW()
            WHERE id = $1 AND tenant_id = $2
            "#,
        )
        .bind(id)
        .bind(tenant_id)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn increment_error_count(&self, id: uuid::Uuid, tenant_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE subscriptions
            SET error_count = error_count + 1, updated_at = NOW()
            WHERE id = $1 AND tenant_id = $2
            "#,
        )
        .bind(id)
        .bind(tenant_id)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn reset_error_count(&self, id: uuid::Uuid, tenant_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE subscriptions
            SET error_count = 0, updated_at = NOW()
            WHERE id = $1 AND tenant_id = $2
            "#,
        )
        .bind(id)
        .bind(tenant_id)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }
}

/// Convert a database row to a SubscriptionSource
fn row_to_subscription(row: &PgRow) -> Result<cogkos_core::models::SubscriptionSource> {
    use sqlx::Row;

    let config: serde_json::Value = row
        .try_get("config")
        .map_err(|e| CogKosError::Database(e.to_string()))?;
    let claimant: serde_json::Value = row
        .try_get("claimant_template")
        .map_err(|e| CogKosError::Database(e.to_string()))?;
    let source_type_str: String = row
        .try_get("source_type")
        .map_err(|e| CogKosError::Database(e.to_string()))?;

    Ok(cogkos_core::models::SubscriptionSource {
        id: row
            .try_get("id")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        tenant_id: row
            .try_get("tenant_id")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        name: row
            .try_get("name")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        source_type: parse_subscription_type(&source_type_str),
        config: serde_json::from_value(config).map_err(CogKosError::Serialization)?,
        poll_interval_secs: row.try_get::<i64, _>("poll_interval_secs").unwrap_or(3600) as u64,
        claimant_template: serde_json::from_value(claimant).map_err(CogKosError::Serialization)?,
        base_confidence: row
            .try_get("base_confidence")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        enabled: row
            .try_get("enabled")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        last_polled: row.try_get("last_polled").ok(),
        error_count: row.try_get::<i64, _>("error_count").unwrap_or(0) as u32,
    })
}

/// Parse subscription type from string
fn parse_subscription_type(s: &str) -> cogkos_core::models::SubscriptionType {
    match s.to_lowercase().as_str() {
        "rss" => cogkos_core::models::SubscriptionType::Rss,
        "webhook" => cogkos_core::models::SubscriptionType::Webhook,
        "api" => cogkos_core::models::SubscriptionType::Api,
        _ => cogkos_core::models::SubscriptionType::Rss,
    }
}
