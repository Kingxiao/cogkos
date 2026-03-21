//! Minimal LLM client trait for paradigm shift framework generation.
//!
//! This defines the core trait only. Concrete implementations live in `cogkos-llm`.
//! The paradigm_shift sandbox accepts `Arc<dyn LlmClient>` — callers inject
//! a cogkos-llm client (or any adapter) at construction time.

/// Minimal LLM client trait — avoids cogkos-core depending on cogkos-llm.
///
/// cogkos-llm provides the concrete `OpenAiClient` / `AnthropicClient`; pass
/// one wrapped in `LlmClientAdapter` (below) to `FrameworkSandbox::with_llm_client`.
#[async_trait::async_trait]
pub trait LlmClient: Send + Sync {
    async fn chat(&self, request: LlmRequest) -> std::result::Result<LlmResponse, LlmClientError>;
}

/// Errors from LLM client
#[derive(Debug, thiserror::Error)]
pub enum LlmClientError {
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Rate limited, retry after {0} seconds")]
    RateLimited(u64),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    #[error("Network error: {0}")]
    NetworkError(String),
}

/// LLM request (self-contained, no dependency on cogkos-llm types)
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub model: String,
    pub messages: Vec<LlmMessage>,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    pub top_p: Option<f32>,
    pub stop_sequences: Vec<String>,
}

/// LLM message
#[derive(Debug, Clone)]
pub struct LlmMessage {
    pub role: LlmRole,
    pub content: String,
}

#[derive(Debug, Clone)]
pub enum LlmRole {
    System,
    User,
    Assistant,
}

/// LLM response
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub usage: Option<LlmUsage>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LlmUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}
