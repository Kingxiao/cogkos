//! CogKOS MCP - MCP Server implementation

pub mod auth;
pub mod cache;
pub mod merger;
pub mod server;
pub mod tools;

pub use auth::*;
pub use cache::*;
pub use server::*;

/// MCP transport mode
#[derive(Clone, Debug, Default, PartialEq)]
pub enum McpTransport {
    /// stdio transport (1 process = 1 agent)
    #[default]
    Stdio,
    /// Streamable HTTP transport (1 server = N agents)
    StreamableHttp,
}

/// MCP Server configuration
#[derive(Clone, Debug)]
pub struct McpConfig {
    pub host: String,
    pub port: u16,
    pub max_connections: usize,
    pub cache_ttl_seconds: i64,
    pub cache_max_entries: usize,
    pub rate_limit_per_minute: Option<u32>,
    pub transport: McpTransport,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 3000,
            max_connections: 10000,
            cache_ttl_seconds: 3600,
            cache_max_entries: 10000,
            rate_limit_per_minute: Some(600),
            transport: McpTransport::default(),
        }
    }
}
