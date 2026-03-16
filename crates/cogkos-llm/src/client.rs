use crate::error::{LlmError, Result};
use crate::types::*;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::{Client, header};
use serde::Deserialize;
use std::pin::Pin;
use std::time::Duration;
use tracing::{debug, error, warn};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn chat(&self, request: LlmRequest) -> Result<LlmResponse>;

    async fn chat_stream(
        &self,
        request: LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>>;

    async fn embed(&self, texts: Vec<String>, model: Option<String>) -> Result<Vec<Vec<f32>>>;

    fn provider(&self) -> &'static str;
}

/// Placeholder client for testing purposes
pub struct PlaceholderClient;

#[async_trait]
impl LlmClient for PlaceholderClient {
    async fn chat(&self, _request: LlmRequest) -> Result<LlmResponse> {
        Ok(LlmResponse {
            content: String::new(),
            usage: Some(Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            }),
            finish_reason: Some("stop".to_string()),
        })
    }

    async fn chat_stream(
        &self,
        _request: LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        let stream = futures::stream::iter(vec![Ok(String::new())]);
        Ok(Box::pin(stream))
    }

    async fn embed(&self, _texts: Vec<String>, _model: Option<String>) -> Result<Vec<Vec<f32>>> {
        Ok(vec![])
    }

    fn provider(&self) -> &'static str {
        "placeholder"
    }
}

pub struct OpenAiClient {
    client: Client,
    api_key: SecretString,
    base_url: String,
    default_model: String,
}

/// Wrapper that zeroizes the inner String on drop
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
struct SecretString(String);

impl SecretString {
    fn expose(&self) -> &str {
        &self.0
    }
}

impl OpenAiClient {
    pub fn new(api_key: String) -> Result<Self> {
        Self::with_base_url(api_key, "https://api.openai.com/v1".to_string())
    }

    pub fn with_base_url(api_key: String, base_url: String) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;

        Ok(Self {
            client,
            api_key: SecretString(api_key),
            base_url,
            default_model: "gpt-4".to_string(),
        })
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.default_model = model;
        self
    }

    fn build_headers(&self) -> Result<header::HeaderMap> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {}", self.api_key.expose()))
                .map_err(|e| LlmError::AuthError(format!("Invalid API key header: {}", e)))?,
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        Ok(headers)
    }
}

#[async_trait]
impl LlmClient for OpenAiClient {
    async fn chat(&self, request: LlmRequest) -> Result<LlmResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        let body = ChatCompletionRequest {
            model: request.model.clone(),
            messages: request.messages,
            temperature: Some(request.temperature),
            max_tokens: request.max_tokens,
            top_p: request.top_p,
            stream: Some(false),
            stop: if request.stop_sequences.is_empty() {
                None
            } else {
                Some(request.stop_sequences)
            },
        };

        debug!("Sending chat request to OpenAI: {}", url);

        let response = self
            .client
            .post(&url)
            .headers(self.build_headers()?)
            .json(&body)
            .send()
            .await?;

        let status = response.status();

        if status == 429 {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(60);
            return Err(LlmError::RateLimited(retry_after));
        }

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!("OpenAI API error: {} - {}", status, error_text);
            return Err(LlmError::ApiError(format!("{}: {}", status, error_text)));
        }

        let completion: ChatCompletionResponse = response.json().await?;

        let choice = completion
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::InvalidResponse("No choices in response".to_string()))?;

        Ok(LlmResponse {
            content: choice.message.content,
            usage: completion.usage,
            finish_reason: choice.finish_reason,
        })
    }

    async fn chat_stream(
        &self,
        request: LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        let url = format!("{}/chat/completions", self.base_url);

        let body = ChatCompletionRequest {
            model: request.model.clone(),
            messages: request.messages,
            temperature: Some(request.temperature),
            max_tokens: request.max_tokens,
            top_p: request.top_p,
            stream: Some(true),
            stop: if request.stop_sequences.is_empty() {
                None
            } else {
                Some(request.stop_sequences)
            },
        };

        debug!("Sending streaming chat request to OpenAI");

        let response = self
            .client
            .post(&url)
            .headers(self.build_headers()?)
            .json(&body)
            .send()
            .await?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(LlmError::ApiError(format!("{}: {}", status, error_text)));
        }

        let stream = response
            .bytes_stream()
            .filter_map(|result| async move {
                match result {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
                        let mut contents = Vec::new();

                        for line in lines {
                            if let Some(data) = line.strip_prefix("data: ") {
                                if data == "[DONE]" {
                                    continue;
                                }

                                match serde_json::from_str::<StreamChunk>(data) {
                                    Ok(chunk) => {
                                        for choice in chunk.choices {
                                            if let Some(content) = choice.delta.content {
                                                contents.push(Ok(content));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to parse SSE data: {}", e);
                                    }
                                }
                            }
                        }

                        Some(futures::stream::iter(contents))
                    }
                    Err(e) => Some(futures::stream::iter(vec![Err(LlmError::ReqwestError(e))])),
                }
            })
            .flatten();

        Ok(Box::pin(stream))
    }

    async fn embed(&self, texts: Vec<String>, model: Option<String>) -> Result<Vec<Vec<f32>>> {
        let is_minimax = self.base_url.contains("minimax");
        let is_302ai = self.base_url.contains("302");

        let body = if is_minimax {
            // MiniMax format - requires texts + type: query
            EmbeddingRequest {
                model: model.unwrap_or_else(|| "embo-01".to_string()),
                input: None,
                texts: Some(texts),
                r#type: Some("query".to_string()),
            }
        } else if is_302ai {
            // 302.ai - OpenAI compatible format
            EmbeddingRequest {
                model: model.unwrap_or_else(|| "text-embedding-3-small".to_string()),
                input: Some(texts),
                texts: None,
                r#type: None,
            }
        } else {
            // OpenAI format
            EmbeddingRequest {
                model: model.unwrap_or_else(|| "text-embedding-3-small".to_string()),
                input: Some(texts),
                texts: None,
                r#type: None,
            }
        };

        let url = format!("{}/embeddings", self.base_url);

        debug!("Sending embedding request to {}", url);

        let response = self
            .client
            .post(&url)
            .headers(self.build_headers()?)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(LlmError::ApiError(error_text));
        }

        let embedding_response: EmbeddingResponse = response.json().await?;

        // Handle both OpenAI and MiniMax response formats
        let embeddings: Vec<Vec<f32>> = if let Some(data) = embedding_response.data {
            // OpenAI format
            data.into_iter().map(|d| d.embedding).collect()
        } else if let Some(vectors) = embedding_response.vectors {
            // MiniMax format
            vectors
        } else {
            return Err(LlmError::InvalidResponse(
                "No embedding data in response".to_string(),
            ));
        };

        Ok(embeddings)
    }

    fn provider(&self) -> &'static str {
        "openai"
    }
}

pub struct AnthropicClient {
    client: Client,
    api_key: SecretString,
    base_url: String,
    default_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnthropicResponse {
    id: String,
    #[serde(rename = "type")]
    response_type: String,
    role: String,
    content: Vec<AnthropicContent>,
    model: String,
    usage: AnthropicUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

impl AnthropicClient {
    pub fn new(api_key: String) -> Result<Self> {
        Self::with_base_url(api_key, "https://api.anthropic.com".to_string())
    }

    pub fn with_base_url(api_key: String, base_url: String) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;

        Ok(Self {
            client,
            api_key: SecretString(api_key),
            base_url,
            default_model: "claude-3-sonnet-20240229".to_string(),
        })
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.default_model = model;
        self
    }

    fn build_headers(&self) -> Result<header::HeaderMap> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            "x-api-key",
            header::HeaderValue::from_str(self.api_key.expose())
                .map_err(|e| LlmError::AuthError(format!("Invalid API key header: {}", e)))?,
        );
        headers.insert(
            "anthropic-version",
            header::HeaderValue::from_static("2023-06-01"),
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        Ok(headers)
    }
}

#[async_trait]
impl LlmClient for AnthropicClient {
    async fn chat(&self, request: LlmRequest) -> Result<LlmResponse> {
        let url = format!("{}/v1/messages", self.base_url);

        let messages: Vec<AnthropicMessage> = request
            .messages
            .into_iter()
            .map(|m| AnthropicMessage {
                role: match m.role {
                    crate::types::Role::System => "system".to_string(),
                    crate::types::Role::User => "user".to_string(),
                    crate::types::Role::Assistant => "assistant".to_string(),
                    crate::types::Role::Function => "assistant".to_string(),
                },
                content: m.content,
            })
            .collect();

        let body = AnthropicRequest {
            model: request.model.clone(),
            max_tokens: request.max_tokens.unwrap_or(4096),
            messages,
            temperature: Some(request.temperature),
            top_p: request.top_p,
            stream: Some(false),
        };

        debug!("Sending chat request to Anthropic");

        let response = self
            .client
            .post(&url)
            .headers(self.build_headers()?)
            .json(&body)
            .send()
            .await?;

        let status = response.status();

        if status == 429 {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(60);
            return Err(LlmError::RateLimited(retry_after));
        }

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!("Anthropic API error: {} - {}", status, error_text);
            return Err(LlmError::ApiError(format!("{}: {}", status, error_text)));
        }

        let anthropic_response: AnthropicResponse = response.json().await?;

        let content = anthropic_response
            .content
            .into_iter()
            .filter(|c| c.content_type == "text")
            .map(|c| c.text)
            .collect::<Vec<_>>()
            .join("");

        Ok(LlmResponse {
            content,
            usage: Some(Usage {
                prompt_tokens: anthropic_response.usage.input_tokens,
                completion_tokens: anthropic_response.usage.output_tokens,
                total_tokens: anthropic_response.usage.input_tokens
                    + anthropic_response.usage.output_tokens,
            }),
            finish_reason: Some("stop".to_string()),
        })
    }

    async fn chat_stream(
        &self,
        request: LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        let url = format!("{}/v1/messages", self.base_url);

        let messages: Vec<AnthropicMessage> = request
            .messages
            .into_iter()
            .map(|m| AnthropicMessage {
                role: match m.role {
                    crate::types::Role::System => "system".to_string(),
                    crate::types::Role::User => "user".to_string(),
                    crate::types::Role::Assistant => "assistant".to_string(),
                    crate::types::Role::Function => "assistant".to_string(),
                },
                content: m.content,
            })
            .collect();

        let body = AnthropicRequest {
            model: request.model.clone(),
            max_tokens: request.max_tokens.unwrap_or(4096),
            messages,
            temperature: Some(request.temperature),
            top_p: request.top_p,
            stream: Some(true),
        };

        debug!("Sending streaming chat request to Anthropic");

        let response = self
            .client
            .post(&url)
            .headers(self.build_headers()?)
            .json(&body)
            .send()
            .await?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(LlmError::ApiError(format!("{}: {}", status, error_text)));
        }

        // Anthropic SSE format:
        //   event: content_block_delta
        //   data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}
        let stream = response
            .bytes_stream()
            .filter_map(|result| async move {
                match result {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        let mut contents = Vec::new();

                        for line in text.lines() {
                            if let Some(data) = line.strip_prefix("data: ")
                                && let Ok(event) = serde_json::from_str::<serde_json::Value>(data)
                                && event.get("type").and_then(|t| t.as_str())
                                    == Some("content_block_delta")
                                && let Some(text) = event
                                    .get("delta")
                                    .and_then(|d| d.get("text"))
                                    .and_then(|t| t.as_str())
                            {
                                contents.push(Ok(text.to_string()));
                            }
                        }

                        Some(futures::stream::iter(contents))
                    }
                    Err(e) => Some(futures::stream::iter(vec![Err(LlmError::ReqwestError(e))])),
                }
            })
            .flatten();

        Ok(Box::pin(stream))
    }

    async fn embed(&self, _texts: Vec<String>, _model: Option<String>) -> Result<Vec<Vec<f32>>> {
        Err(LlmError::ApiError(
            "Anthropic does not provide embeddings API".to_string(),
        ))
    }

    fn provider(&self) -> &'static str {
        "anthropic"
    }
}

use serde::Serialize;
