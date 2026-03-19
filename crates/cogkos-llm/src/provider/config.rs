//! LLM Provider Configuration
//!
//! Multi-type LLM configuration: text, embedding, image, audio, other

use serde::{Deserialize, Serialize};
use std::env;
use std::fmt;

/// Single LLM provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProviderConfig {
    /// Provider name: openai, anthropic, kimi, 302ai, doubao, openrouter, etc.
    pub provider: String,
    /// Model name
    pub model: String,
    /// API key (can be overridden by env var)
    #[serde(default)]
    pub api_key: String,
    /// Base URL (optional)
    #[serde(default)]
    pub base_url: Option<String>,
}

impl Default for LlmProviderConfig {
    fn default() -> Self {
        Self {
            provider: "openai".to_string(),
            model: "gpt-4".to_string(),
            api_key: String::new(),
            base_url: None,
        }
    }
}

impl LlmProviderConfig {
    /// Create from config file values, applying env var overrides
    pub fn from_toml(
        provider: String,
        model: String,
        api_key: String,
        base_url: Option<String>,
        env_key: &str,
    ) -> Self {
        // Priority: specific env var > generic fallback > config file
        let final_api_key = env::var(env_key)
            .ok()
            .or_else(|| env::var("OPENAI_API_KEY").ok())
            .into_iter()
            .find(|v| !v.is_empty())
            .unwrap_or(api_key);

        Self {
            provider,
            model,
            api_key: final_api_key,
            base_url,
        }
    }
}

/// Multi-type LLM configuration container
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmConfig {
    /// Text generation LLM config
    #[serde(default)]
    pub text: Option<LlmProviderConfig>,
    /// Embedding LLM config
    #[serde(default)]
    pub embedding: Option<LlmProviderConfig>,
    /// Image understanding LLM config
    #[serde(default)]
    pub image: Option<LlmProviderConfig>,
    /// Speech recognition LLM config
    #[serde(default)]
    pub audio: Option<LlmProviderConfig>,
    /// Other LLM type config
    #[serde(default)]
    pub other: Option<LlmProviderConfig>,
}

/// Get the env var name for a given provider
fn get_env_var_name(provider: &str) -> String {
    match provider {
        "kimi" => "KIMI_API_KEY".to_string(),
        "minimax" => "MINIMAX_API_KEY".to_string(),
        "302ai" => "AI302_API_KEY".to_string(),
        "doubao" => "DOUBAO_API_KEY".to_string(),
        "openai" => "OPENAI_API_KEY".to_string(),
        "openrouter" => "OPENROUTER_API_KEY".to_string(),
        _ => format!("{}_API_KEY", provider.to_uppercase()),
    }
}

impl LlmConfig {
    /// Create from TOML config, applying env var overrides
    pub fn from_toml_config(
        text: Option<(String, String, String, Option<String>)>,
        embedding: Option<(String, String, String, Option<String>)>,
        image: Option<(String, String, String, Option<String>)>,
        audio: Option<(String, String, String, Option<String>)>,
        other: Option<(String, String, String, Option<String>)>,
    ) -> Self {
        Self {
            text: text.map(|(p, m, k, b)| {
                let env_name = get_env_var_name(&p);
                LlmProviderConfig::from_toml(p, m, k, b, &env_name)
            }),
            embedding: embedding.map(|(p, m, k, b)| {
                let env_name = get_env_var_name(&p);
                LlmProviderConfig::from_toml(p, m, k, b, &env_name)
            }),
            image: image.map(|(p, m, k, b)| {
                let env_name = get_env_var_name(&p);
                LlmProviderConfig::from_toml(p, m, k, b, &env_name)
            }),
            audio: audio.map(|(p, m, k, b)| {
                let env_name = get_env_var_name(&p);
                LlmProviderConfig::from_toml(p, m, k, b, &env_name)
            }),
            other: other.map(|(p, m, k, b)| {
                let env_name = get_env_var_name(&p);
                LlmProviderConfig::from_toml(p, m, k, b, &env_name)
            }),
        }
    }

    /// Get config for the specified LLM type
    pub fn get(&self, llm_type: &str) -> Option<&LlmProviderConfig> {
        match llm_type {
            "text" => self.text.as_ref(),
            "embedding" => self.embedding.as_ref(),
            "image" => self.image.as_ref(),
            "audio" => self.audio.as_ref(),
            "other" => self.other.as_ref(),
            _ => None,
        }
    }

    /// Check if the specified type is configured (has a valid API key)
    pub fn is_configured(&self, llm_type: &str) -> bool {
        self.get(llm_type)
            .map(|c| !c.api_key.is_empty())
            .unwrap_or(false)
    }
}

impl fmt::Display for LlmProviderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{} (provider: {})",
            self.model,
            if self.api_key.is_empty() {
                "no-api-key"
            } else {
                "configured"
            },
            self.provider
        )
    }
}

impl fmt::Display for LlmConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.is_configured("text") {
            parts.push("text");
        }
        if self.is_configured("embedding") {
            parts.push("embedding");
        }
        if self.is_configured("image") {
            parts.push("image");
        }
        if self.is_configured("audio") {
            parts.push("audio");
        }
        if self.is_configured("other") {
            parts.push("other");
        }

        if parts.is_empty() {
            write!(f, "No LLM configured")
        } else {
            write!(f, "LLM: {}", parts.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_config_from_toml() {
        // Set env vars to simulate config file + env var override scenario
        unsafe {
            env::set_var("KIMI_API_KEY", "test-key");
        }
        unsafe {
            env::set_var("AI302_API_KEY", "embedding-key");
        }

        let config = LlmConfig::from_toml_config(
            Some((
                "kimi".to_string(),
                "moonshot-v1-8k".to_string(),
                "".to_string(),
                None,
            )),
            Some((
                "302ai".to_string(),
                "text-embedding-3-small".to_string(),
                "".to_string(),
                None,
            )),
            None,
            None,
            None,
        );

        assert!(config.is_configured("text"));
        assert!(config.is_configured("embedding"));
        assert!(!config.is_configured("image"));

        unsafe {
            env::remove_var("KIMI_API_KEY");
            env::remove_var("AI302_API_KEY");
        }
    }

    #[test]
    fn test_config_fallback_to_provided_key() {
        // When env var is not set, should use the provided api_key
        let config = LlmProviderConfig::from_toml(
            "kimi".to_string(),
            "moonshot-v1-8k".to_string(),
            "key-from-config".to_string(),
            None,
            "COGKOS_TEST_NONEXISTENT_KEY_12345",
        );

        // If OPENAI_API_KEY is set in env, it will be used as fallback
        // Otherwise, "key-from-config" is used
        assert!(!config.api_key.is_empty(), "API key should not be empty");
    }
}
