use cogkos_core::Result;
use cogkos_store::AuthStore;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Authentication context
#[derive(Clone, Debug)]
pub struct AuthContext {
    pub tenant_id: String,
    pub permissions: Vec<String>,
    pub api_key_hash: String,
}

impl AuthContext {
    /// Check if has permission
    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.contains(&permission.to_string())
            || self.permissions.contains(&"*".to_string())
    }

    /// Check if can read
    pub fn can_read(&self) -> bool {
        self.has_permission("read")
    }

    /// Check if can write
    pub fn can_write(&self) -> bool {
        self.has_permission("write")
    }
}

/// API key cache entry
#[derive(Clone)]
struct CacheEntry {
    tenant_id: String,
    permissions: Vec<String>,
    cached_at: chrono::DateTime<chrono::Utc>,
}

/// Authentication middleware
pub struct AuthMiddleware {
    auth_store: Arc<dyn AuthStore>,
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    cache_ttl_seconds: i64,
}

impl AuthMiddleware {
    /// Create new auth middleware
    pub fn new(auth_store: Arc<dyn AuthStore>, cache_ttl_seconds: i64) -> Self {
        Self {
            auth_store,
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl_seconds,
        }
    }

    /// Authenticate request.
    ///
    /// Dev mode shortcut: if `DEFAULT_MCP_API_KEY` is set and the provided key
    /// matches, return a synthetic AuthContext without hitting the database.
    /// Tenant comes from `DEFAULT_MCP_TENANT` env var (defaults to "default").
    pub async fn authenticate(&self, api_key: &str) -> Result<AuthContext> {
        // Dev mode: bypass DB when using the default key
        if let Ok(default_key) = std::env::var("DEFAULT_MCP_API_KEY") {
            if !default_key.is_empty() && api_key == default_key {
                let tenant =
                    std::env::var("DEFAULT_MCP_TENANT").unwrap_or_else(|_| "default".to_string());
                return Ok(AuthContext {
                    tenant_id: tenant,
                    permissions: vec!["read".to_string(), "write".to_string()],
                    api_key_hash: self.hash_key(api_key),
                });
            }
        }

        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(api_key) {
                let age = chrono::Utc::now() - entry.cached_at;
                if age.num_seconds() < self.cache_ttl_seconds {
                    return Ok(AuthContext {
                        tenant_id: entry.tenant_id.clone(),
                        permissions: entry.permissions.clone(),
                        api_key_hash: self.hash_key(api_key),
                    });
                }
            }
        }

        // Validate with store
        let (tenant_id, permissions) = self.auth_store.validate_api_key(api_key).await?;

        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(
                api_key.to_string(),
                CacheEntry {
                    tenant_id: tenant_id.clone(),
                    permissions: permissions.clone(),
                    cached_at: chrono::Utc::now(),
                },
            );
        }

        Ok(AuthContext {
            tenant_id,
            permissions,
            api_key_hash: self.hash_key(api_key),
        })
    }

    /// Hash API key for logging (not for storage)
    pub(crate) fn hash_key(&self, key: &str) -> String {
        use hex::encode;
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(key);
        encode(&hasher.finalize()[..8]) // First 8 bytes only
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cogkos_store::InMemoryAuthStore;

    fn make_context(permissions: Vec<&str>) -> AuthContext {
        AuthContext {
            tenant_id: "test-tenant".to_string(),
            permissions: permissions.into_iter().map(String::from).collect(),
            api_key_hash: "testhash".to_string(),
        }
    }

    #[test]
    fn auth_context_has_permission_exact_match() {
        let ctx = make_context(vec!["read", "write"]);
        assert!(ctx.has_permission("read"));
        assert!(ctx.has_permission("write"));
    }

    #[test]
    fn auth_context_has_permission_wildcard() {
        let ctx = make_context(vec!["*"]);
        assert!(ctx.has_permission("read"));
        assert!(ctx.has_permission("write"));
        assert!(ctx.has_permission("admin"));
    }

    #[test]
    fn auth_context_has_permission_missing() {
        let ctx = make_context(vec!["read"]);
        assert!(!ctx.has_permission("write"));
        assert!(!ctx.has_permission("admin"));
    }

    #[test]
    fn auth_context_can_read_with_read_perm() {
        let ctx = make_context(vec!["read"]);
        assert!(ctx.can_read());
    }

    #[test]
    fn auth_context_can_read_without_perm() {
        let ctx = make_context(vec!["write"]);
        assert!(!ctx.can_read());
    }

    #[test]
    fn auth_context_can_write_with_write_perm() {
        let ctx = make_context(vec!["write"]);
        assert!(ctx.can_write());
    }

    #[test]
    fn auth_context_can_write_without_perm() {
        let ctx = make_context(vec!["read"]);
        assert!(!ctx.can_write());
    }

    #[tokio::test]
    async fn auth_middleware_authenticate_caches() {
        let store = InMemoryAuthStore::new();
        let auth_store: Arc<dyn AuthStore> = Arc::new(store);

        // Register a key via the store trait
        let api_key = auth_store
            .create_api_key("tenant-1", vec!["read".to_string(), "write".to_string()])
            .await
            .unwrap();

        let middleware = AuthMiddleware::new(Arc::clone(&auth_store), 3600);

        // First call populates cache
        let ctx1 = middleware.authenticate(&api_key).await.unwrap();
        assert_eq!(ctx1.tenant_id, "tenant-1");
        assert!(ctx1.can_read());
        assert!(ctx1.can_write());

        // Second call should hit cache and return same result
        let ctx2 = middleware.authenticate(&api_key).await.unwrap();
        assert_eq!(ctx2.tenant_id, ctx1.tenant_id);
        assert_eq!(ctx2.permissions, ctx1.permissions);

        // Verify hash_key is consistent
        assert_eq!(ctx1.api_key_hash, ctx2.api_key_hash);
    }
}
