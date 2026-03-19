//! LLM Provider Configuration Module
//!
//! Multi-type LLM configuration: text, embedding, image, audio, other
//! Priority: config file > env vars > defaults

pub mod config;

use std::env;

/// LLM type alias
pub type LlmType = &'static str;

/// LLM config types
pub const LLM_TYPES: &[&str] = &["text", "embedding", "image", "audio", "other"];

/// Map env var name to LLM type
pub fn env_key_for_llm_type(llm_type: &str) -> Option<&'static str> {
    match llm_type {
        "text" => Some("KIMI_API_KEY"),
        "embedding" => Some("AI302_API_KEY"),
        "image" => Some("DOUBAO_API_KEY"),
        "audio" => Some("OPENAI_API_KEY"),
        "other" => Some("OPENROUTER_API_KEY"),
        _ => None,
    }
}

/// Get API key from env var
pub fn get_api_key_from_env(llm_type: &str) -> Option<String> {
    env_key_for_llm_type(llm_type)
        .and_then(|key| env::var(key).ok())
        .filter(|v| !v.is_empty())
}
