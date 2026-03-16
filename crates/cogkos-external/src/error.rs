use thiserror::Error;

#[derive(Error, Debug)]
pub enum ExternalError {
    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("API error ({provider}): {message}")]
    ApiError { provider: String, message: String },

    #[error("Rate limited: retry after {0}s")]
    RateLimited(u64),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Timeout")]
    Timeout,

    #[error("Invalid parameters: {0}")]
    InvalidParams(String),

    #[error("Search engine error: {0}")]
    SearchError(String),

    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),

    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),

    #[error(transparent)]
    UrlError(#[from] url::ParseError),

    #[error(transparent)]
    FeedParseError(#[from] feed_rs::parser::ParseFeedError),
}

pub type Result<T> = std::result::Result<T, ExternalError>;
