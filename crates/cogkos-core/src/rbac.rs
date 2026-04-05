//! Role-Based Access Control (RBAC) module
//!
//! Provides:
//! - Role definitions
//! - Permission management
//! - Resource-level access control

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Role definitions
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    /// Super administrator - full access
    Admin,
    /// Can edit and query knowledge
    Editor,
    /// Can only query knowledge
    Viewer,
    /// Can manage external subscriptions
    Subscriber,
    /// Custom role
    Custom(String),
}

impl Role {
    /// Get default permissions for role
    pub fn default_permissions(&self) -> Vec<Permission> {
        match self {
            Role::Admin => vec![
                Permission::ReadKnowledge,
                Permission::WriteKnowledge,
                Permission::DeleteKnowledge,
                Permission::ManageUsers,
                Permission::ManageSubscriptions,
                Permission::ViewAudit,
                Permission::SystemConfig,
            ],
            Role::Editor => vec![
                Permission::ReadKnowledge,
                Permission::WriteKnowledge,
                Permission::ViewAudit,
            ],
            Role::Viewer => vec![Permission::ReadKnowledge],
            Role::Subscriber => vec![Permission::ReadKnowledge, Permission::ManageSubscriptions],
            Role::Custom(_) => vec![Permission::ReadKnowledge],
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::Admin => write!(f, "admin"),
            Role::Editor => write!(f, "editor"),
            Role::Viewer => write!(f, "viewer"),
            Role::Subscriber => write!(f, "subscriber"),
            Role::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// Permission types
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    /// Read knowledge entries
    ReadKnowledge,
    /// Write knowledge entries
    WriteKnowledge,
    /// Delete knowledge entries
    DeleteKnowledge,
    /// Manage users and roles
    ManageUsers,
    /// Manage subscriptions
    ManageSubscriptions,
    /// View audit logs
    ViewAudit,
    /// Modify system configuration
    SystemConfig,
    /// Query federated knowledge
    FederatedQuery,
    /// Upload documents
    UploadDocument,
    /// Manage gaps
    ManageGaps,
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Permission::ReadKnowledge => write!(f, "read:knowledge"),
            Permission::WriteKnowledge => write!(f, "write:knowledge"),
            Permission::DeleteKnowledge => write!(f, "delete:knowledge"),
            Permission::ManageUsers => write!(f, "manage:users"),
            Permission::ManageSubscriptions => write!(f, "manage:subscriptions"),
            Permission::ViewAudit => write!(f, "view:audit"),
            Permission::SystemConfig => write!(f, "system:config"),
            Permission::FederatedQuery => write!(f, "query:federated"),
            Permission::UploadDocument => write!(f, "upload:document"),
            Permission::ManageGaps => write!(f, "manage:gaps"),
        }
    }
}

/// Resource types that can be protected
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceType {
    /// Knowledge entries
    Knowledge,
    /// Documents
    Document,
    /// Subscriptions
    Subscription,
    /// Audit logs
    AuditLog,
    /// System configuration
    SystemConfig,
    /// Users and roles
    User,
    /// Knowledge gaps
    Gap,
    /// Custom resource
    Custom(String),
}

/// Access scope
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AccessScope {
    /// Can access own tenant's resources
    Tenant,
    /// Can access specific resource ID
    Resource(String),
    /// Can access all resources
    Global,
}

/// Role assignment for a user
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoleAssignment {
    pub user_id: String,
    pub role: Role,
    pub scope: AccessScope,
    pub tenant_id: String,
}

/// Access policy
#[derive(Clone, Debug)]
pub struct AccessPolicy {
    pub role: Role,
    pub permission: Permission,
    pub resource: ResourceType,
    pub scope: AccessScope,
}

impl AccessPolicy {
    pub fn new(role: Role, permission: Permission, resource: ResourceType) -> Self {
        Self {
            role,
            permission,
            resource,
            scope: AccessScope::Tenant,
        }
    }

    pub fn with_scope(mut self, scope: AccessScope) -> Self {
        self.scope = scope;
        self
    }
}

/// RBAC context for authorization
#[derive(Clone)]
pub struct RbacContext {
    pub user_id: String,
    pub roles: Vec<Role>,
    pub tenant_id: String,
}

impl RbacContext {
    pub fn new(user_id: impl Into<String>, tenant_id: impl Into<String>) -> Self {
        Self {
            user_id: user_id.into(),
            roles: vec![Role::Viewer],
            tenant_id: tenant_id.into(),
        }
    }

    pub fn with_roles(mut self, roles: Vec<Role>) -> Self {
        self.roles = roles;
        self
    }

    /// Check if user has a specific permission
    pub fn has_permission(&self, permission: &Permission) -> bool {
        for role in &self.roles {
            if role.default_permissions().contains(permission) {
                return true;
            }
        }
        false
    }

    /// Check if user has a specific role
    pub fn has_role(&self, role: &Role) -> bool {
        self.roles.contains(role)
    }

    /// Check if user can access a resource
    pub fn can_access(&self, permission: &Permission, _resource: &ResourceType) -> bool {
        self.has_permission(permission)
    }
}

/// RBAC authorization engine
#[derive(Clone)]
pub struct RbacEngine {
    policies: Arc<RwLock<Vec<AccessPolicy>>>,
    role_assignments: Arc<RwLock<HashMap<String, Vec<RoleAssignment>>>>,
}

impl RbacEngine {
    pub fn new() -> Self {
        Self {
            policies: Arc::new(RwLock::new(Vec::new())),
            role_assignments: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add an access policy
    pub fn add_policy(&self, policy: AccessPolicy) {
        self.policies.write().push(policy);
    }

    /// Assign a role to a user
    pub fn assign_role(&self, assignment: RoleAssignment) {
        let key = format!("{}:{}", assignment.tenant_id, assignment.user_id);
        self.role_assignments
            .write()
            .entry(key)
            .or_default()
            .push(assignment);
    }

    /// Get user context
    pub fn get_context(&self, user_id: &str, tenant_id: &str) -> RbacContext {
        let key = format!("{}:{}", tenant_id, user_id);
        let assignments = self.role_assignments.read();

        let roles: Vec<Role> = assignments
            .get(&key)
            .map(|assignments| assignments.iter().map(|a| a.role.clone()).collect())
            .unwrap_or_else(|| vec![Role::Viewer]);

        RbacContext::new(user_id, tenant_id).with_roles(roles)
    }

    /// Check if user can perform action
    pub fn authorize(
        &self,
        user_id: &str,
        tenant_id: &str,
        permission: &Permission,
        resource: &ResourceType,
    ) -> bool {
        let context = self.get_context(user_id, tenant_id);

        // Check direct permission
        if context.can_access(permission, resource) {
            return true;
        }

        // Check policies
        let policies = self.policies.read();
        for policy in policies.iter() {
            if context.has_role(&policy.role)
                && &policy.permission == permission
                && &policy.resource == resource
            {
                return true;
            }
        }

        false
    }

    /// Create default RBAC setup
    pub fn with_defaults() -> Self {
        let engine = Self::new();

        // Add default policies for Admin
        engine.add_policy(
            AccessPolicy::new(
                Role::Admin,
                Permission::ReadKnowledge,
                ResourceType::Knowledge,
            )
            .with_scope(AccessScope::Global),
        );

        engine.add_policy(
            AccessPolicy::new(
                Role::Admin,
                Permission::WriteKnowledge,
                ResourceType::Knowledge,
            )
            .with_scope(AccessScope::Global),
        );

        engine.add_policy(
            AccessPolicy::new(
                Role::Admin,
                Permission::DeleteKnowledge,
                ResourceType::Knowledge,
            )
            .with_scope(AccessScope::Global),
        );

        // Add default policies for Editor
        engine.add_policy(AccessPolicy::new(
            Role::Editor,
            Permission::ReadKnowledge,
            ResourceType::Knowledge,
        ));

        engine.add_policy(AccessPolicy::new(
            Role::Editor,
            Permission::WriteKnowledge,
            ResourceType::Knowledge,
        ));

        // Add default policies for Viewer
        engine.add_policy(AccessPolicy::new(
            Role::Viewer,
            Permission::ReadKnowledge,
            ResourceType::Knowledge,
        ));

        engine
    }
}

impl Default for RbacEngine {
    fn default() -> Self {
        Self::new()
    }
}

lazy_static::lazy_static! {
    pub static ref RBAC: RbacEngine = RbacEngine::with_defaults();
}

/// Check if user can perform action
pub fn authorize(
    user_id: &str,
    tenant_id: &str,
    permission: Permission,
    resource: ResourceType,
) -> bool {
    RBAC.authorize(user_id, tenant_id, &permission, &resource)
}

/// Get user context
pub fn get_context(user_id: &str, tenant_id: &str) -> RbacContext {
    RBAC.get_context(user_id, tenant_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_permissions() {
        let admin = Role::Admin;
        let permissions = admin.default_permissions();
        assert!(permissions.contains(&Permission::WriteKnowledge));
        assert!(permissions.contains(&Permission::DeleteKnowledge));

        let viewer = Role::Viewer;
        let permissions = viewer.default_permissions();
        assert!(permissions.contains(&Permission::ReadKnowledge));
        assert!(!permissions.contains(&Permission::DeleteKnowledge));
    }

    #[test]
    fn test_rbac_context() {
        let context = RbacContext::new("user1", "tenant1").with_roles(vec![Role::Editor]);

        assert!(context.has_permission(&Permission::ReadKnowledge));
        assert!(context.has_permission(&Permission::WriteKnowledge));
        assert!(!context.has_permission(&Permission::DeleteKnowledge));
    }

    #[test]
    fn test_rbac_engine() {
        let engine = RbacEngine::new();

        // Assign admin role
        engine.assign_role(RoleAssignment {
            user_id: "admin1".to_string(),
            role: Role::Admin,
            scope: AccessScope::Global,
            tenant_id: "tenant1".to_string(),
        });

        // Authorize
        assert!(engine.authorize(
            "admin1",
            "tenant1",
            &Permission::WriteKnowledge,
            &ResourceType::Knowledge
        ));

        // Viewer cannot delete
        let viewer_context = RbacContext::new("viewer1", "tenant1").with_roles(vec![Role::Viewer]);
        assert!(!viewer_context.has_permission(&Permission::DeleteKnowledge));
    }
}
