use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::claim::Claimant;

/// Tenant identifier type alias
pub type TenantId = String;

/// API Key model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: Uuid,
    pub key_hash: String,
    pub tenant_id: TenantId,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used: Option<chrono::DateTime<chrono::Utc>>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_active: bool,
}

impl ApiKey {
    /// Create a new API key
    pub fn new(key_hash: impl Into<String>, tenant_id: TenantId) -> Self {
        Self {
            id: Uuid::new_v4(),
            key_hash: key_hash.into(),
            tenant_id,
            created_at: chrono::Utc::now(),
            last_used: None,
            expires_at: None,
            is_active: true,
        }
    }

    /// Mark the key as used
    pub fn mark_used(&mut self) {
        self.last_used = Some(chrono::Utc::now());
    }

    /// Check if the key is valid
    pub fn is_valid(&self) -> bool {
        self.is_active && self.expires_at.is_none_or(|exp| exp > chrono::Utc::now())
    }
}

/// Access envelope for permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessEnvelope {
    pub visibility: Visibility,

    #[serde(default)]
    pub allowed_roles: Vec<String>,
    #[serde(default)]
    pub gdpr_applicable: bool,
}

impl AccessEnvelope {
    /// Create a new access envelope for a tenant
    pub fn new(_tenant_id: impl Into<String>) -> Self {
        Self {
            visibility: Visibility::Tenant,

            allowed_roles: Vec::new(),
            gdpr_applicable: false,
        }
    }

    /// Create access envelope based on claimant source
    ///
    /// Inference rules:
    /// - Human/Agent/System → Tenant (owner's data)
    /// - ExternalPublic → Public (publicly available data)
    pub fn from_claimant(_tenant_id: impl Into<String>, claimant: &Claimant) -> Self {
        let visibility = match claimant {
            // User's own data - Tenant visibility
            Claimant::Human { .. } => Visibility::Tenant,
            // Agent-generated data - belongs to the tenant
            Claimant::Agent { .. } => Visibility::Tenant,
            // System data - internal to tenant
            Claimant::System => Visibility::Tenant,
            // External public data - can be shared
            Claimant::ExternalPublic { .. } => Visibility::Public,
        };

        Self {
            visibility,

            allowed_roles: Vec::new(),
            gdpr_applicable: matches!(claimant, Claimant::ExternalPublic { .. }),
        }
    }

    /// Set visibility
    pub fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }

    /// Add allowed role
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.allowed_roles.push(role.into());
        self
    }

    /// Set GDPR applicability
    pub fn with_gdpr(mut self, applicable: bool) -> Self {
        self.gdpr_applicable = applicable;
        self
    }

    /// Check if the given tenant and roles can access
    pub fn can_access(&self, _tenant_id: &str, roles: &[String]) -> bool {
        match self.visibility {
            Visibility::Public => true,
            Visibility::CrossTenant => true,
            Visibility::Tenant => true,
            Visibility::Team => {
                self.allowed_roles.is_empty()
                    || roles.iter().any(|r| self.allowed_roles.contains(r))
            }
            Visibility::Private => {
                !self.allowed_roles.is_empty()
                    && roles.iter().any(|r| self.allowed_roles.contains(r))
            }
        }
    }
}

/// Visibility level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Visibility {
    Private,
    Team,
    Tenant,
    CrossTenant,
    Public,
}
