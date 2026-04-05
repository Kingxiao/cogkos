//! Audit logging module
//!
//! Provides:
//! - Structured audit log entries
//! - Event categorization (auth, data, system, security)
//! - Log persistence interface
//! - Query/filter capabilities

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Audit event categories
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AuditCategory {
    /// Authentication events (login, logout, token refresh)
    Authentication,
    /// Authorization events (permission denied, access granted)
    Authorization,
    /// Data operations (create, read, update, delete)
    DataOperation,
    /// System events (startup, shutdown, config changes)
    #[default]
    System,
    /// Security events (attacks, suspicious activity)
    Security,
    /// API requests
    ApiRequest,
    /// Custom category
    Custom(String),
}

/// Audit event severity
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AuditSeverity {
    Debug,
    #[default]
    Info,
    Warning,
    Error,
    Critical,
}

/// Audit actor (who performed the action)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditActor {
    /// User ID
    pub user_id: Option<String>,
    /// API key hash
    pub api_key_hash: Option<String>,
    /// Service/agent ID
    pub service_id: Option<String>,
    /// IP address
    pub ip_address: Option<String>,
    /// User agent
    pub user_agent: Option<String>,
}

impl AuditActor {
    pub fn new() -> Self {
        Self {
            user_id: None,
            api_key_hash: None,
            service_id: None,
            ip_address: None,
            user_agent: None,
        }
    }

    pub fn with_user_id(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn with_api_key(mut self, hash: String) -> Self {
        self.api_key_hash = Some(hash);
        self
    }

    pub fn with_service(mut self, service_id: String) -> Self {
        self.service_id = Some(service_id);
        self
    }

    pub fn with_ip(mut self, ip: String) -> Self {
        self.ip_address = Some(ip);
        self
    }
}

impl Default for AuditActor {
    fn default() -> Self {
        Self::new()
    }
}

/// Audit target (what was affected)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditTarget {
    /// Resource type (e.g., "claim", "document", "user")
    pub resource_type: String,
    /// Resource ID
    pub resource_id: Option<String>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl AuditTarget {
    pub fn new(resource_type: impl Into<String>) -> Self {
        Self {
            resource_type: resource_type.into(),
            resource_id: None,
            metadata: HashMap::new(),
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.resource_id = Some(id.into());
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Audit outcome
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AuditOutcome {
    #[default]
    Success,
    Failure,
    Partial,
    Pending,
}

/// Audit log entry
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Unique entry ID
    pub id: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Event category
    pub category: AuditCategory,
    /// Severity level
    pub severity: AuditSeverity,
    /// Action performed
    pub action: String,
    /// Actor who performed the action
    pub actor: AuditActor,
    /// Target resource
    pub target: Option<AuditTarget>,
    /// Outcome
    pub outcome: AuditOutcome,
    /// Error message if failure
    pub error_message: Option<String>,
    /// Additional details
    pub details: HashMap<String, String>,
    /// Request ID for tracing
    pub request_id: Option<String>,
    /// Tenant ID
    pub tenant_id: Option<String>,
}

impl AuditEntry {
    pub fn new(action: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            category: AuditCategory::default(),
            severity: AuditSeverity::default(),
            action: action.into(),
            actor: AuditActor::new(),
            target: None,
            outcome: AuditOutcome::default(),
            error_message: None,
            details: HashMap::new(),
            request_id: None,
            tenant_id: None,
        }
    }

    pub fn with_category(mut self, category: AuditCategory) -> Self {
        self.category = category;
        self
    }

    pub fn with_severity(mut self, severity: AuditSeverity) -> Self {
        self.severity = severity;
        self
    }

    pub fn with_actor(mut self, actor: AuditActor) -> Self {
        self.actor = actor;
        self
    }

    pub fn with_target(mut self, target: AuditTarget) -> Self {
        self.target = Some(target);
        self
    }

    pub fn with_outcome(mut self, outcome: AuditOutcome) -> Self {
        self.outcome = outcome;
        self
    }

    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error_message = Some(error.into());
        self.outcome = AuditOutcome::Failure;
        self.severity = AuditSeverity::Error;
        self
    }

    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }

    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    pub fn with_tenant_id(mut self, tenant_id: impl Into<String>) -> Self {
        self.tenant_id = Some(tenant_id.into());
        self
    }
}

/// Audit log storage backend
#[async_trait]
pub trait AuditStore: Send + Sync {
    /// Write an audit entry
    async fn write(&self, entry: AuditEntry) -> Result<(), String>;

    /// Query audit entries
    async fn query(&self, filter: AuditFilter, limit: usize) -> Result<Vec<AuditEntry>, String>;
}

/// Audit filter for querying
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AuditFilter {
    pub tenant_id: Option<String>,
    pub category: Option<AuditCategory>,
    pub severity: Option<AuditSeverity>,
    pub action: Option<String>,
    pub actor_id: Option<String>,
    pub resource_type: Option<String>,
    pub outcome: Option<AuditOutcome>,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
}

/// In-memory audit store
pub struct InMemoryAuditStore {
    entries: Arc<RwLock<Vec<AuditEntry>>>,
    max_entries: usize,
}

impl InMemoryAuditStore {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::new())),
            max_entries,
        }
    }

    pub fn with_default_capacity() -> Self {
        Self::new(10000)
    }

    /// Get entries for direct access
    pub fn entries(&self) -> &Arc<RwLock<Vec<AuditEntry>>> {
        &self.entries
    }

    /// Sync write (blocking)
    pub fn write_sync(&self, entry: AuditEntry) {
        let mut entries = self.entries.write();
        entries.push(entry);
        while entries.len() > self.max_entries {
            entries.remove(0);
        }
    }
}

#[async_trait]
impl AuditStore for InMemoryAuditStore {
    async fn write(&self, entry: AuditEntry) -> Result<(), String> {
        let mut entries = self.entries.write();
        entries.push(entry);
        // Evict oldest if over capacity
        while entries.len() > self.max_entries {
            entries.remove(0);
        }
        Ok(())
    }

    async fn query(&self, filter: AuditFilter, limit: usize) -> Result<Vec<AuditEntry>, String> {
        let entries = self.entries.read();
        let mut results: Vec<AuditEntry> = entries
            .iter()
            .filter(|e| {
                if let Some(ref tid) = filter.tenant_id
                    && e.tenant_id.as_ref() != Some(tid)
                {
                    return false;
                }
                if let Some(ref cat) = filter.category
                    && &e.category != cat
                {
                    return false;
                }
                if let Some(ref sev) = filter.severity
                    && &e.severity != sev
                {
                    return false;
                }
                if let Some(ref act) = filter.action
                    && !e.action.contains(act)
                {
                    return false;
                }
                if let Some(ref aid) = filter.actor_id
                    && e.actor.user_id.as_ref() != Some(aid)
                    && e.actor.api_key_hash.as_ref() != Some(aid)
                {
                    return false;
                }
                if let Some(ref rt) = filter.resource_type
                    && e.target.as_ref().map(|t| &t.resource_type) != Some(rt)
                {
                    return false;
                }
                if let Some(ref out) = filter.outcome
                    && &e.outcome != out
                {
                    return false;
                }
                if let Some(ref start) = filter.start_time
                    && e.timestamp < *start
                {
                    return false;
                }
                if let Some(ref end) = filter.end_time
                    && e.timestamp > *end
                {
                    return false;
                }
                true
            })
            .cloned()
            .collect();

        // Sort by timestamp descending
        results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        results.truncate(limit);
        Ok(results)
    }
}

/// Global audit logger - using Arc for shared state
pub static AUDIT_LOG: std::sync::OnceLock<InMemoryAuditStore> = std::sync::OnceLock::new();

fn get_audit_store() -> &'static InMemoryAuditStore {
    AUDIT_LOG.get_or_init(InMemoryAuditStore::with_default_capacity)
}

/// Log an audit event
pub fn log_audit(entry: AuditEntry) {
    // Log to the global store (blocking write)
    let store = get_audit_store();
    store.write_sync(entry.clone());

    // Also log to tracing for visibility
    match entry.outcome {
        AuditOutcome::Success => {
            tracing::info!(
                audit.action = %entry.action,
                audit.category = ?entry.category,
                audit.actor = ?entry.actor,
                audit.target = ?entry.target,
                "Audit event"
            );
        }
        AuditOutcome::Failure => {
            tracing::warn!(
                audit.action = %entry.action,
                audit.category = ?entry.category,
                audit.error = ?entry.error_message,
                "Audit event failed"
            );
        }
        _ => {
            tracing::debug!(
                audit.action = %entry.action,
                audit.category = ?entry.category,
                "Audit event"
            );
        }
    }
}

/// Helper to create authentication audit entries
pub fn audit_auth(
    action: &str,
    actor: AuditActor,
    outcome: AuditOutcome,
    tenant_id: Option<&str>,
) -> AuditEntry {
    let mut entry = AuditEntry::new(action)
        .with_category(AuditCategory::Authentication)
        .with_actor(actor)
        .with_outcome(outcome);

    if let Some(tid) = tenant_id {
        entry = entry.with_tenant_id(tid);
    }

    entry
}

/// Helper to create data operation audit entries
pub fn audit_data(
    action: &str,
    actor: AuditActor,
    target: AuditTarget,
    outcome: AuditOutcome,
    tenant_id: Option<&str>,
) -> AuditEntry {
    let mut entry = AuditEntry::new(action)
        .with_category(AuditCategory::DataOperation)
        .with_actor(actor)
        .with_target(target)
        .with_outcome(outcome);

    if let Some(tid) = tenant_id {
        entry = entry.with_tenant_id(tid);
    }

    entry
}

/// Helper to create security audit entries
pub fn audit_security(
    action: &str,
    actor: AuditActor,
    outcome: AuditOutcome,
    error: Option<&str>,
    tenant_id: Option<&str>,
) -> AuditEntry {
    let mut entry = AuditEntry::new(action)
        .with_category(AuditCategory::Security)
        .with_severity(AuditSeverity::Warning)
        .with_actor(actor)
        .with_outcome(outcome);

    if let Some(e) = error {
        entry = entry.with_error(e);
    }

    if let Some(tid) = tenant_id {
        entry = entry.with_tenant_id(tid);
    }

    entry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_entry_builder() {
        let entry = AuditEntry::new("user.login")
            .with_category(AuditCategory::Authentication)
            .with_severity(AuditSeverity::Info)
            .with_actor(
                AuditActor::new()
                    .with_user_id("user123".to_string())
                    .with_ip("192.168.1.1".to_string()),
            )
            .with_target(AuditTarget::new("session").with_id("sess_abc"))
            .with_outcome(AuditOutcome::Success)
            .with_tenant_id("tenant_xyz");

        assert_eq!(entry.action, "user.login");
        assert_eq!(entry.category, AuditCategory::Authentication);
        assert_eq!(entry.actor.user_id, Some("user123".to_string()));
        assert_eq!(entry.target.as_ref().unwrap().resource_type, "session");
        assert_eq!(entry.outcome, AuditOutcome::Success);
        assert_eq!(entry.tenant_id, Some("tenant_xyz".to_string()));
    }

    #[tokio::test]
    async fn test_in_memory_audit_store() {
        let store = InMemoryAuditStore::new(100);

        let entry = AuditEntry::new("test.action")
            .with_tenant_id("tenant1")
            .with_category(AuditCategory::DataOperation);

        store.write(entry.clone()).await.unwrap();

        let results = store
            .query(
                AuditFilter {
                    tenant_id: Some("tenant1".to_string()),
                    ..Default::default()
                },
                10,
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "test.action");
    }

    #[tokio::test]
    async fn test_audit_filter() {
        let store = InMemoryAuditStore::new(100);

        // Write multiple entries
        for i in 0..5 {
            let entry = AuditEntry::new(format!("action_{}", i))
                .with_category(AuditCategory::DataOperation)
                .with_tenant_id("tenant1");
            store.write(entry).await.unwrap();
        }

        for i in 0..3 {
            let entry = AuditEntry::new(format!("auth_{}", i))
                .with_category(AuditCategory::Authentication)
                .with_tenant_id("tenant1");
            store.write(entry).await.unwrap();
        }

        // Filter by category
        let results = store
            .query(
                AuditFilter {
                    category: Some(AuditCategory::Authentication),
                    ..Default::default()
                },
                10,
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 3);
    }
}
