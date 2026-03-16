use thiserror::Error;

#[derive(Error, Debug)]
pub enum CogKosError {
    // Client errors (4xx)
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Permission denied: {0}")]
    Forbidden(String),

    #[error("Access denied: {0}")]
    AccessDenied(String),

    #[error("External service error: {0}")]
    ExternalError(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Tenant not found: {0}")]
    TenantNotFound(String),

    #[error("Rate limited")]
    RateLimited,

    // Server errors (5xx)
    #[error("Database error: {0}")]
    Database(String),

    #[error("Graph database error: {0}")]
    Graph(String),

    #[error("Vector store error: {0}")]
    Vector(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl CogKosError {
    /// Get error code for API responses
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "NOT_FOUND",
            Self::Forbidden(_) => "FORBIDDEN",
            Self::AccessDenied(_) => "ACCESS_DENIED",
            Self::ExternalError(_) => "EXTERNAL_ERROR",
            Self::InvalidInput(_) => "INVALID_INPUT",
            Self::TenantNotFound(_) => "TENANT_NOT_FOUND",
            Self::RateLimited => "RATE_LIMITED",
            Self::Database(_) => "DATABASE_ERROR",
            Self::Graph(_) => "GRAPH_ERROR",
            Self::Vector(_) => "VECTOR_ERROR",
            Self::Storage(_) => "STORAGE_ERROR",
            Self::Serialization(_) => "SERIALIZATION_ERROR",
            Self::Internal(_) => "INTERNAL_ERROR",
            Self::Auth(_) => "AUTH_ERROR",
            Self::Config(_) => "CONFIG_ERROR",
            Self::Parse(_) => "PARSE_ERROR",
            Self::Io(_) => "IO_ERROR",
        }
    }

    /// Get HTTP status code
    pub fn status_code(&self) -> u16 {
        match self {
            Self::NotFound(_) => 404,
            Self::Forbidden(_) => 403,
            Self::AccessDenied(_) => 403,
            Self::ExternalError(_) => 502,
            Self::InvalidInput(_) | Self::TenantNotFound(_) => 400,
            Self::RateLimited => 429,
            Self::Auth(_) => 401,
            _ => 500,
        }
    }
}

/// Result type for CogKOS
pub type Result<T> = std::result::Result<T, CogKosError>;
