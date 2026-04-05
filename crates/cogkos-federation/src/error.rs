use thiserror::Error;

#[derive(Error, Debug)]
pub enum FederationError {
    #[error("Node not found: {0}")]
    NodeNotFound(String),

    #[error("Node unavailable: {0}")]
    NodeUnavailable(String),

    #[error("Routing failed: {0}")]
    RoutingError(String),

    #[error("Aggregation failed: {0}")]
    AggregationError(String),

    #[error("Query timeout")]
    QueryTimeout,

    #[error("Invalid response from node {0}: {1}")]
    InvalidResponse(String, String),

    #[error("Insufficient nodes available: {0}/{1}")]
    InsufficientNodes(usize, usize),

    #[error("Consensus not reached")]
    ConsensusNotReached,

    #[error("Cross-instance routing failed: {0}")]
    CrossInstanceError(String),

    #[error("Metadata directory error: {0}")]
    MetadataDirectoryError(String),

    #[error("Federation not enabled")]
    NotEnabled,

    #[error(transparent)]
    NetworkError(#[from] reqwest::Error),

    #[error(transparent)]
    SerializationError(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, FederationError>;
