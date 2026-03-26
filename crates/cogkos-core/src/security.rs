//! CogKOS security mode — controls all security behaviors from one config point.
//!
//! Set `COGKOS_ENV=production` to enable production security controls.
//! Default is development mode for backward compatibility.

/// Security mode controlling authentication, CORS, and audit behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityMode {
    /// Development: convenience over security (DEFAULT_MCP_API_KEY works, CORS open)
    Development,
    /// Production: security enforced (no dev key bypass, CORS restricted)
    Production,
}

impl SecurityMode {
    /// Resolve security mode from `COGKOS_ENV` environment variable.
    ///
    /// - `"production"` or `"prod"` → [`SecurityMode::Production`]
    /// - Anything else (including unset) → [`SecurityMode::Development`]
    pub fn from_env() -> Self {
        match std::env::var("COGKOS_ENV").as_deref() {
            Ok("production") | Ok("prod") => Self::Production,
            _ => Self::Development,
        }
    }

    pub fn is_production(&self) -> bool {
        *self == Self::Production
    }

    pub fn is_development(&self) -> bool {
        *self == Self::Development
    }
}

impl std::fmt::Display for SecurityMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Development => write!(f, "development"),
            Self::Production => write!(f, "production"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_development() {
        // When COGKOS_ENV is not set, should default to Development
        // SAFETY: test runs single-threaded; no concurrent env access
        unsafe { std::env::remove_var("COGKOS_ENV") };
        assert_eq!(SecurityMode::from_env(), SecurityMode::Development);
    }

    #[test]
    fn display_modes() {
        assert_eq!(SecurityMode::Development.to_string(), "development");
        assert_eq!(SecurityMode::Production.to_string(), "production");
    }

    #[test]
    fn is_helpers() {
        assert!(SecurityMode::Development.is_development());
        assert!(!SecurityMode::Development.is_production());
        assert!(SecurityMode::Production.is_production());
        assert!(!SecurityMode::Production.is_development());
    }
}
