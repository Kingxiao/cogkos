//! AuthStore implementation for PostgresStore

use super::PostgresStore;
use async_trait::async_trait;
use cogkos_core::{CogKosError, Result};
use sqlx::Row;

#[async_trait]
impl crate::AuthStore for PostgresStore {
    async fn validate_api_key(&self, api_key: &str) -> Result<(String, Vec<String>)> {
        let result = sqlx::query(
            "SELECT tenant_id, permissions FROM api_keys
             WHERE key_hash = crypt($1, key_hash) AND enabled = true
             AND (expires_at IS NULL OR expires_at > NOW())",
        )
        .bind(api_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        match result {
            Some(row) => {
                let tenant_id: String = row.get("tenant_id");
                let permissions: Vec<String> = row.get("permissions");

                Ok((tenant_id, permissions))
            }
            None => Err(CogKosError::AccessDenied("Invalid API key".to_string())),
        }
    }

    async fn create_api_key(&self, tenant_id: &str, permissions: Vec<String>) -> Result<String> {
        let api_key = uuid::Uuid::new_v4().to_string();

        sqlx::query(
            "INSERT INTO api_keys (key_hash, tenant_id, name, permissions)
             VALUES (crypt($1, gen_salt('bf')), $2, 'auto-generated', $3)",
        )
        .bind(&api_key)
        .bind(tenant_id)
        .bind(&permissions)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(api_key)
    }

    async fn revoke_api_key(&self, key_hash: &str) -> Result<()> {
        sqlx::query("UPDATE api_keys SET enabled = false WHERE key_hash = $1")
            .bind(key_hash)
            .execute(&self.pool)
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Ok(())
    }
}
