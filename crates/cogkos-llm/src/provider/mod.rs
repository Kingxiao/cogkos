//! LLM Provider Configuration Module
//!
//! 支持多类型 LLM 配置：text, embedding, image, audio, other
//! 配置优先级：配置文件 > 环境变量 > 默认值

pub mod config;

use std::env;

/// LLM 类型别名
pub type LlmType = &'static str;

/// LLM 配置类型
pub const LLM_TYPES: &[&str] = &["text", "embedding", "image", "audio", "other"];

/// 环境变量映射到 LLM 类型
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

/// 从环境变量获取 API Key
pub fn get_api_key_from_env(llm_type: &str) -> Option<String> {
    env_key_for_llm_type(llm_type)
        .and_then(|key| env::var(key).ok())
        .filter(|v| !v.is_empty())
}
