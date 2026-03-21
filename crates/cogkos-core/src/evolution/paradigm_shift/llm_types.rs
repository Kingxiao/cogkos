//! LLM client types for paradigm shift framework generation

use super::ParadigmShiftError;

/// LLM client trait - defined locally to avoid circular dependencies
/// This trait abstracts LLM API calls for framework generation
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

/// LLM request
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

/// OpenAI-compatible LLM client
pub struct OpenAiLlmClient {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    default_model: String,
}

impl OpenAiLlmClient {
    pub fn new(api_key: String) -> super::Result<Self> {
        Self::with_base_url(api_key, "https://api.openai.com/v1".to_string()) // verified: 2026-03-21
    }

    pub fn with_base_url(api_key: String, base_url: String) -> super::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| ParadigmShiftError::SandboxError(format!("HTTP client: {}", e)))?;

        Ok(Self {
            client,
            api_key,
            base_url,
            default_model: "gpt-4".to_string(), // verified: 2026-03-21
        })
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.default_model = model;
        self
    }
}

#[async_trait::async_trait]
impl LlmClient for OpenAiLlmClient {
    async fn chat(&self, request: LlmRequest) -> std::result::Result<LlmResponse, LlmClientError> {
        use reqwest::header;

        let url = format!("{}/chat/completions", self.base_url);

        #[derive(serde::Serialize)]
        struct ChatRequest {
            model: String,
            messages: Vec<serde_json::Value>,
            temperature: Option<f32>,
            max_tokens: Option<u32>,
            top_p: Option<f32>,
            stream: bool,
        }

        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": match m.role {
                        LlmRole::System => "system",
                        LlmRole::User => "user",
                        LlmRole::Assistant => "assistant",
                    },
                    "content": m.content
                })
            })
            .collect();

        let body = ChatRequest {
            model: request.model,
            messages,
            temperature: Some(request.temperature),
            max_tokens: request.max_tokens,
            top_p: request.top_p,
            stream: false,
        };

        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                .map_err(|e| LlmClientError::ApiError(e.to_string()))?,
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmClientError::NetworkError(e.to_string()))?;

        let status = response.status();

        if status == 429 {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(60);
            return Err(LlmClientError::RateLimited(retry_after));
        }

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(LlmClientError::ApiError(format!(
                "{}: {}",
                status, error_text
            )));
        }

        #[derive(serde::Deserialize)]
        struct ChatResponse {
            choices: Vec<serde_json::Value>,
            usage: Option<serde_json::Value>,
        }

        let completion: ChatResponse = response
            .json()
            .await
            .map_err(|e| LlmClientError::InvalidResponse(e.to_string()))?;

        let content = completion
            .choices
            .first()
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        let usage = completion.usage.map(|u| LlmUsage {
            prompt_tokens: u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            completion_tokens: u
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        });

        Ok(LlmResponse {
            content,
            usage,
            finish_reason: None,
        })
    }
}
