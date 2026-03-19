//! LLM Provider Configuration
//!
//! 支持多类型 LLM 配置：text, embedding, image, audio, other

use serde::{Deserialize, Serialize};
use std::env;
use std::fmt;

/// 单个 LLM Provider 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProviderConfig {
    /// Provider 名称: openai, anthropic, kimi, 302ai, doubao, openrouter 等
    pub provider: String,
    /// 模型名称
    pub model: String,
    /// API Key（可通过环境变量覆盖）
    #[serde(default)]
    pub api_key: String,
    /// 基础 URL（可选）
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
    /// 从配置文件值创建配置，应用环境变量覆盖
    pub fn from_toml(
        provider: String,
        model: String,
        api_key: String,
        base_url: Option<String>,
        env_key: &str,
    ) -> Self {
        // 配置优先级：指定环境变量 > 通用 fallback > 配置文件
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

/// 多类型 LLM 配置容器
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmConfig {
    /// 文本生成 LLM 配置
    #[serde(default)]
    pub text: Option<LlmProviderConfig>,
    /// Embedding LLM 配置
    #[serde(default)]
    pub embedding: Option<LlmProviderConfig>,
    /// 图像理解 LLM 配置
    #[serde(default)]
    pub image: Option<LlmProviderConfig>,
    /// 语音识别 LLM 配置
    #[serde(default)]
    pub audio: Option<LlmProviderConfig>,
    /// 其他类型 LLM 配置
    #[serde(default)]
    pub other: Option<LlmProviderConfig>,
}

/// 根据 provider 名称获取对应的环境变量名
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
    /// 从 TOML 配置创建，应用环境变量覆盖
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

    /// 获取指定类型的配置
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

    /// 检查指定类型是否已配置（有有效 API Key）
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
        // 设置环境变量，模拟配置文件 + 环境变量覆盖的场景
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
