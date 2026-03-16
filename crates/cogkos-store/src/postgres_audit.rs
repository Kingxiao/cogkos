//! PostgreSQL Audit Store implementation

use async_trait::async_trait;
use cogkos_core::audit::{
    AuditActor, AuditCategory, AuditEntry, AuditFilter, AuditOutcome, AuditSeverity, AuditStore,
    AuditTarget,
};
use sqlx::{PgPool, Row, postgres::PgRow};
use std::collections::HashMap;
use std::result::Result as StdResult;
use std::sync::Arc;

/// PostgreSQL Audit Store - persists audit logs to PostgreSQL
pub struct PostgresAuditStore {
    pool: PgPool,
}

impl PostgresAuditStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Create from pool reference (Arc)
    pub fn from_pool(pool: Arc<PgPool>) -> Self {
        Self {
            pool: (*pool).clone(),
        }
    }

}

#[async_trait]
impl AuditStore for PostgresAuditStore {
    async fn write(&self, entry: AuditEntry) -> StdResult<(), String> {
        let category = format!("{:?}", entry.category);
        let severity = format!("{:?}", entry.severity);
        let outcome = format!("{:?}", entry.outcome);

        // Serialize target metadata
        let target_metadata = entry
            .target
            .as_ref()
            .map(|t| serde_json::to_value(&t.metadata).unwrap_or_default())
            .unwrap_or_default();

        // Serialize details
        let details = match serde_json::to_value(&entry.details) {
            Ok(v) => v,
            Err(e) => return Err(format!("Failed to serialize details: {}", e)),
        };

        // Get target fields
        let (target_resource_type, target_resource_id) = entry
            .target
            .as_ref()
            .map(|t| (Some(t.resource_type.clone()), t.resource_id.clone()))
            .unwrap_or((None, None));

        sqlx::query(
            r#"
            INSERT INTO audit_logs (
                id, timestamp, category, severity, action,
                actor_user_id, actor_api_key_hash, actor_service_id, actor_ip_address, actor_user_agent,
                target_resource_type, target_resource_id, target_metadata,
                outcome, error_message, details, request_id, tenant_id
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
            "#,
        )
        .bind(entry.id)
        .bind(entry.timestamp)
        .bind(category)
        .bind(severity)
        .bind(entry.action)
        .bind(entry.actor.user_id)
        .bind(entry.actor.api_key_hash)
        .bind(entry.actor.service_id)
        .bind(entry.actor.ip_address)
        .bind(entry.actor.user_agent)
        .bind(target_resource_type)
        .bind(target_resource_id)
        .bind(target_metadata)
        .bind(outcome)
        .bind(entry.error_message)
        .bind(details)
        .bind(entry.request_id)
        .bind(entry.tenant_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to write audit log: {}", e))?;

        Ok(())
    }

    async fn query(&self, filter: AuditFilter, limit: usize) -> StdResult<Vec<AuditEntry>, String> {
        // Build query with optional filters
        // Use simple approach with COALESCE for optional parameters
        let rows = sqlx::query(
            r#"
            SELECT id, timestamp, category, severity, action,
                   actor_user_id, actor_api_key_hash, actor_service_id, actor_ip_address, actor_user_agent,
                   target_resource_type, target_resource_id, target_metadata,
                   outcome, error_message, details, request_id, tenant_id
            FROM audit_logs
            WHERE ($1 IS NULL OR $1 = '' OR tenant_id = $1)
              AND ($2 IS NULL OR $2 = '' OR category = $2)
              AND ($3 IS NULL OR $3 = '' OR severity = $3)
              AND ($4 IS NULL OR $4 = '' OR action LIKE '%' || $4 || '%')
              AND ($5 IS NULL OR $5 = '' OR actor_user_id = $5 OR actor_api_key_hash = $5)
              AND ($6 IS NULL OR $6 = '' OR target_resource_type = $6)
              AND ($7 IS NULL OR $7 = '' OR outcome = $7)
              AND ($8 IS NULL OR timestamp >= $8)
              AND ($9 IS NULL OR timestamp <= $9)
            ORDER BY timestamp DESC
            LIMIT $10
            "#,
        )
        .bind(filter.tenant_id.clone().unwrap_or_default())
        .bind(filter.category.as_ref().map(|c| format!("{:?}", c)).unwrap_or_default())
        .bind(filter.severity.as_ref().map(|s| format!("{:?}", s)).unwrap_or_default())
        .bind(filter.action.clone().unwrap_or_default())
        .bind(filter.actor_id.clone().unwrap_or_default())
        .bind(filter.resource_type.clone().unwrap_or_default())
        .bind(filter.outcome.as_ref().map(|o| format!("{:?}", o)).unwrap_or_default())
        .bind(filter.start_time)
        .bind(filter.end_time)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to query audit logs: {}", e))?;

        rows.iter().map(row_to_audit_entry).collect()
    }
}

/// Convert a database row to an AuditEntry
fn row_to_audit_entry(row: &PgRow) -> StdResult<AuditEntry, String> {
    let category_str: String = row
        .try_get("category")
        .unwrap_or_else(|_| "System".to_string());
    let severity_str: String = row
        .try_get("severity")
        .unwrap_or_else(|_| "Info".to_string());
    let outcome_str: String = row
        .try_get("outcome")
        .unwrap_or_else(|_| "Success".to_string());

    let target_metadata: serde_json::Value = row.try_get("target_metadata").unwrap_or_default();

    let details: serde_json::Value = row.try_get("details").unwrap_or_default();

    let details_map: HashMap<String, String> = details
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    // Build target if resource_type exists
    let target = row
        .try_get::<Option<String>, _>("target_resource_type")
        .ok()
        .flatten()
        .map(|rt| {
            let resource_id = row
                .try_get::<Option<String>, _>("target_resource_id")
                .ok()
                .flatten();

            let metadata: HashMap<String, String> = target_metadata
                .as_object()
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();

            AuditTarget {
                resource_type: rt,
                resource_id,
                metadata,
            }
        });

    Ok(AuditEntry {
        id: row
            .try_get("id")
            .map_err(|e| format!("Failed to get id: {}", e))?,
        timestamp: row
            .try_get("timestamp")
            .map_err(|e| format!("Failed to get timestamp: {}", e))?,
        category: parse_category(&category_str),
        severity: parse_severity(&severity_str),
        action: row
            .try_get("action")
            .map_err(|e| format!("Failed to get action: {}", e))?,
        actor: AuditActor {
            user_id: row.try_get("actor_user_id").ok(),
            api_key_hash: row.try_get("actor_api_key_hash").ok(),
            service_id: row.try_get("actor_service_id").ok(),
            ip_address: row.try_get("actor_ip_address").ok(),
            user_agent: row.try_get("actor_user_agent").ok(),
        },
        target,
        outcome: parse_outcome(&outcome_str),
        error_message: row.try_get("error_message").ok(),
        details: details_map,
        request_id: row.try_get("request_id").ok(),
        tenant_id: row.try_get("tenant_id").ok(),
    })
}

fn parse_category(s: &str) -> AuditCategory {
    match s {
        "Authentication" => AuditCategory::Authentication,
        "Authorization" => AuditCategory::Authorization,
        "DataOperation" => AuditCategory::DataOperation,
        "System" => AuditCategory::System,
        "Security" => AuditCategory::Security,
        "ApiRequest" => AuditCategory::ApiRequest,
        _ => AuditCategory::Custom(s.to_string()),
    }
}

fn parse_severity(s: &str) -> AuditSeverity {
    match s {
        "Debug" => AuditSeverity::Debug,
        "Info" => AuditSeverity::Info,
        "Warning" => AuditSeverity::Warning,
        "Error" => AuditSeverity::Error,
        "Critical" => AuditSeverity::Critical,
        _ => AuditSeverity::Info,
    }
}

fn parse_outcome(s: &str) -> AuditOutcome {
    match s {
        "Success" => AuditOutcome::Success,
        "Failure" => AuditOutcome::Failure,
        "Partial" => AuditOutcome::Partial,
        "Pending" => AuditOutcome::Pending,
        _ => AuditOutcome::Success,
    }
}
