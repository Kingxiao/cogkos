//! MCP Server implementation using rmcp SDK

mod handler;
mod rate_limiter;
mod startup;

use std::sync::Arc;

use cogkos_llm::LlmClient;
use cogkos_store::Stores;
use serde::{Deserialize, Serialize};

use crate::{AuthMiddleware, McpConfig, QueryCache};

// Re-export all public items
pub use handler::CogkosMcpHandler;
pub use rate_limiter::RateLimiter;
pub use startup::start_mcp_server;

/// JSON-RPC error (kept for compatibility with tools.rs)
#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }
}

/// MCP Server state
#[derive(Clone)]
pub struct McpServerState {
    pub stores: Stores,
    pub auth: Arc<AuthMiddleware>,
    pub cache: Arc<QueryCache>,
    pub config: McpConfig,
    pub llm_client: Option<Arc<dyn LlmClient>>,
    pub embedding_client: Option<Arc<dyn LlmClient>>,
    pub rate_limiter: RateLimiter,
}

/// Sampling request types (MCP Sampling Protocol)
#[derive(Debug, Deserialize)]
pub struct SamplingRequest {
    #[serde(rename = "samplingType")]
    pub sampling_type: SamplingType,
    pub context: SamplingContext,
    pub prompt: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SamplingType {
    ConflictAnalysis,
    KnowledgeValidation,
    PredictionGeneration,
}

pub fn default_max_tokens() -> u32 {
    2048
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SamplingContext {
    #[serde(default)]
    pub knowledge_items: Vec<KnowledgeItem>,
    #[serde(default)]
    pub conflicts: Vec<ConflictInfo>,
    #[serde(default)]
    pub query_context: Option<String>,
    #[serde(default)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KnowledgeItem {
    pub id: String,
    pub content: String,
    pub confidence: f32,
    pub source: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConflictInfo {
    pub claim_a: String,
    pub claim_b: String,
    pub conflict_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SamplingResponse {
    #[serde(rename = "samplingType")]
    pub sampling_type: String,
    pub content: String,
    pub confidence: f32,
    #[serde(default)]
    pub requires_human_review: bool,
    #[serde(default)]
    pub recommended_actions: Vec<String>,
}

impl SamplingResponse {
    pub fn new(sampling_type: &str, content: String, confidence: f32) -> Self {
        Self {
            sampling_type: sampling_type.to_string(),
            content,
            confidence,
            requires_human_review: false,
            recommended_actions: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_rpc_error_new() {
        let err = JsonRpcError::new(-32600, "Invalid Request");
        assert_eq!(err.code, -32600);
        assert_eq!(err.message, "Invalid Request");
        assert!(err.data.is_none());
    }

    #[test]
    fn json_rpc_error_with_data() {
        let err = JsonRpcError::new(-32600, "err").with_data(serde_json::json!({"detail": "x"}));
        assert!(err.data.is_some());
    }

    #[test]
    fn sampling_response_new() {
        let resp = SamplingResponse::new("conflict_analysis", "result".into(), 0.85);
        assert_eq!(resp.sampling_type, "conflict_analysis");
        assert_eq!(resp.confidence, 0.85);
        assert!(!resp.requires_human_review);
        assert!(resp.recommended_actions.is_empty());
    }

    #[test]
    fn default_max_tokens_value() {
        assert_eq!(default_max_tokens(), 2048);
    }
}
