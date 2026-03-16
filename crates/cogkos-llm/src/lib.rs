pub mod client;
pub mod dedicated_model;
pub mod error;
pub mod prediction;
pub mod provider;
pub mod stream;
pub mod template;
pub mod types;

pub use client::{AnthropicClient, LlmClient, OpenAiClient};
pub use dedicated_model::*;
pub use error::{LlmError, Result};
pub use prediction::{PredictionConfig, PredictionService, PredictionServiceBuilder};
pub use provider::{
    LLM_TYPES, LlmType,
    config::{LlmConfig, LlmProviderConfig},
    env_key_for_llm_type, get_api_key_from_env,
};
pub use stream::{StreamProcessor, StreamingResponse, collect_stream};
pub use template::{PromptTemplate, TemplateManager, build_messages, create_system_prompt};
pub use types::{
    ChatCompletionRequest, ChatCompletionResponse, LlmRequest, LlmResponse, Message, Role, Usage,
};

use std::sync::Arc;
use zeroize::Zeroize;

/// Provider types for LLM clients
#[derive(Debug, Clone, Copy)]
pub enum ProviderType {
    OpenAi,
    Anthropic,
}

/// Builder for creating LLM clients
pub struct LlmClientBuilder {
    api_key: String,
    provider: ProviderType,
    base_url: Option<String>,
    default_model: Option<String>,
}

impl LlmClientBuilder {
    pub fn new(api_key: impl Into<String>, provider: ProviderType) -> Self {
        Self {
            api_key: api_key.into(),
            provider,
            base_url: None,
            default_model: None,
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = Some(model.into());
        self
    }

    pub fn build(&mut self) -> Result<Arc<dyn LlmClient>> {
        let api_key = std::mem::take(&mut self.api_key);
        let base_url = self.base_url.take();
        let default_model = self.default_model.take();

        match self.provider {
            ProviderType::OpenAi => {
                let mut client = match base_url {
                    Some(url) => OpenAiClient::with_base_url(api_key, url)?,
                    None => OpenAiClient::new(api_key)?,
                };

                if let Some(model) = default_model {
                    client = client.with_model(model);
                }

                Ok(Arc::new(client))
            }
            ProviderType::Anthropic => {
                let mut client = match base_url {
                    Some(url) => AnthropicClient::with_base_url(api_key, url)?,
                    None => AnthropicClient::new(api_key)?,
                };

                if let Some(model) = default_model {
                    client = client.with_model(model);
                }

                Ok(Arc::new(client))
            }
        }
    }
}

impl Drop for LlmClientBuilder {
    fn drop(&mut self) {
        self.api_key.zeroize();
    }
}

/// Convenience function to create an OpenAI client
pub fn openai_client(api_key: impl Into<String>) -> Result<Arc<dyn LlmClient>> {
    Ok(Arc::new(OpenAiClient::new(api_key.into())?))
}

/// Convenience function to create an Anthropic client
pub fn anthropic_client(api_key: impl Into<String>) -> Result<Arc<dyn LlmClient>> {
    Ok(Arc::new(AnthropicClient::new(api_key.into())?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_prompt_template() {
        let template =
            PromptTemplate::new("test", "Hello {{name}}, welcome to {{place}}!").unwrap();

        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "World".to_string());
        vars.insert("place".to_string(), "CogKOS".to_string());

        let result = template.render(&vars).unwrap();
        assert_eq!(result, "Hello World, welcome to CogKOS!");
    }

    #[test]
    fn test_template_manager() {
        let manager = TemplateManager::new();

        assert!(manager.get("query_analysis").is_some());
        assert!(manager.get("belief_synthesis").is_some());

        let templates = manager.list();
        assert!(!templates.is_empty());
    }

    #[test]
    fn test_system_prompts() {
        let query_prompt = create_system_prompt("query_analysis");
        assert!(query_prompt.contains("query analysis"));

        let default_prompt = create_system_prompt("unknown");
        assert!(default_prompt.contains("helpful AI assistant"));
    }
}
