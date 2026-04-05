use thiserror::Error;

#[derive(Error, Debug)]
pub enum LlmError {
    #[error("API request failed: {0}")]
    ApiError(String),

    #[error("Authentication failed: {0}")]
    AuthError(String),

    #[error("Rate limited: retry after {0}s")]
    RateLimited(u64),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Request timeout")]
    Timeout,

    #[error("Streaming error: {0}")]
    StreamError(String),

    #[error("Template error: {0}")]
    TemplateError(String),

    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),

    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, LlmError>;
