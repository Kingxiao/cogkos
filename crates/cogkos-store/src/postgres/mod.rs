//! PostgreSQL store implementation

pub mod auth;
pub mod cache;
pub mod claims;
pub mod feedback;
pub mod gaps;
pub mod memory_layers;
pub mod subscriptions;

use cogkos_core::{CogKosError, Result};
use sqlx::PgPool;

/// Validate tenant_id format: must match `[a-z0-9_-]+` (no SQL injection risk).
pub(crate) fn validate_tenant_id(tenant_id: &str) -> Result<()> {
    if tenant_id.is_empty()
        || !tenant_id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    {
        return Err(CogKosError::InvalidInput(
            "Invalid tenant_id format: must match [a-z0-9_-]+".to_string(),
        ));
    }
    Ok(())
}

/// PostgreSQL store
pub struct PostgresStore {
    pub(crate) pool: PgPool,
}

impl PostgresStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Set RLS tenant context on a connection.
    /// Must be called within a transaction for `SET LOCAL` to scope correctly.
    pub(crate) async fn set_tenant_context(
        conn: &mut sqlx::PgConnection,
        tenant_id: &str,
    ) -> Result<()> {
        validate_tenant_id(tenant_id)?;
        // SET LOCAL does not support $1 parameters, so we validate strictly above.
        sqlx::query(&format!("SET LOCAL app.current_tenant = '{}'", tenant_id))
            .execute(&mut *conn)
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Ok(())
    }

    /// Create PostgresStore from database URL
    pub async fn from_url(url: &str) -> Result<Self> {
        let pool = PgPool::connect(url)
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Ok(Self { pool })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_tenant_id_valid() {
        assert!(validate_tenant_id("tenant-1").is_ok());
        assert!(validate_tenant_id("my_org").is_ok());
        assert!(validate_tenant_id("abc123").is_ok());
        assert!(validate_tenant_id("a-b_c-d").is_ok());
    }

    #[test]
    fn test_validate_tenant_id_rejects_empty() {
        assert!(validate_tenant_id("").is_err());
    }

    #[test]
    fn test_validate_tenant_id_rejects_uppercase() {
        assert!(validate_tenant_id("Tenant").is_err());
        assert!(validate_tenant_id("UPPER").is_err());
    }

    #[test]
    fn test_validate_tenant_id_rejects_sql_injection() {
        assert!(validate_tenant_id("'; DROP TABLE --").is_err());
        assert!(validate_tenant_id("tenant' OR '1'='1").is_err());
        assert!(validate_tenant_id("tenant; DELETE FROM").is_err());
    }

    #[test]
    fn test_validate_tenant_id_rejects_special_chars() {
        assert!(validate_tenant_id("tenant.org").is_err());
        assert!(validate_tenant_id("tenant@org").is_err());
        assert!(validate_tenant_id("tenant/org").is_err());
        assert!(validate_tenant_id("tenant org").is_err());
    }
}
