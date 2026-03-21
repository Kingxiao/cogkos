//! Image parsing module for CogKOS
//! Supports image understanding via Vision models

use crate::Result;
use crate::{DocumentParser, TextChunk};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Image parsing configuration
#[derive(Debug, Clone)]
pub struct ImageParserConfig {
    /// Vision model provider (openai, anthropic, etc.)
    pub provider: String,
    /// Vision model name
    pub model: String,
    /// Max tokens for description
    pub max_tokens: u32,
}

impl Default for ImageParserConfig {
    fn default() -> Self {
        Self {
            provider: "openai".to_string(),
            model: std::env::var("IMAGE_LLM_MODEL").unwrap_or_else(|_| "gpt-4o".to_string()), // verified: 2026-03-21
            max_tokens: 1024,
        }
    }
}

/// Image parsing result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageParseResult {
    pub description: String,
    pub entities: Vec<ImageEntity>,
    pub extracted_text: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub format: Option<String>,
}

/// Entity detected in image
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageEntity {
    pub name: String,
    pub entity_type: String,
    pub confidence: f32,
}

/// Image parser for understanding images via Vision models
pub struct ImageParser {
    _config: ImageParserConfig,
}

impl ImageParser {
    pub fn new(_config: ImageParserConfig) -> Self {
        Self { _config }
    }

    /// Analyze image data and return structured result.
    /// Placeholder — production would call a Vision model.
    async fn analyze(&self, image_data: &[u8], fmt: &str) -> Result<ImageParseResult> {
        let (width, height) = self.get_image_dimensions(image_data, fmt)?;

        Ok(ImageParseResult {
            description: "[Vision model integration needed]".to_string(),
            entities: vec![],
            extracted_text: None,
            width: Some(width),
            height: Some(height),
            format: Some(fmt.to_string()),
        })
    }

    fn get_image_dimensions(&self, data: &[u8], fmt: &str) -> Result<(u32, u32)> {
        match fmt.to_lowercase().as_str() {
            "png" => {
                if data.len() > 24 {
                    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
                    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
                    return Ok((width, height));
                }
            }
            "jpeg" | "jpg" => {}
            "gif" => {
                if data.len() > 10 {
                    let width = u16::from_le_bytes([data[6], data[7]]) as u32;
                    let height = u16::from_le_bytes([data[8], data[9]]) as u32;
                    return Ok((width, height));
                }
            }
            _ => {}
        }
        Ok((0, 0))
    }

    pub fn supported_formats() -> Vec<&'static str> {
        vec!["jpg", "jpeg", "png", "gif", "webp", "bmp"]
    }
}

#[async_trait]
impl DocumentParser for ImageParser {
    fn supported_extensions(&self) -> &[&str] {
        &["jpg", "jpeg", "png", "gif", "webp", "bmp"]
    }

    async fn parse(&self, data: &[u8], filename: &str) -> Result<Vec<TextChunk>> {
        let ext = filename
            .rsplit('.')
            .next()
            .unwrap_or("unknown")
            .to_lowercase();

        let r = self.analyze(data, &ext).await?;

        let content = if r.description.contains("[Vision model integration needed]") {
            format!(
                "Image: {}x{} ({})\n\nNote: Vision model integration required for detailed analysis.",
                r.width.unwrap_or(0),
                r.height.unwrap_or(0),
                r.format.as_deref().unwrap_or("unknown")
            )
        } else {
            r.description
        };

        let mut metadata: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        if let Some(w) = r.width {
            metadata.insert("width".to_string(), w.to_string());
        }
        if let Some(h) = r.height {
            metadata.insert("height".to_string(), h.to_string());
        }
        if let Some(f) = r.format {
            metadata.insert("format".to_string(), f);
        }
        if let Some(text) = r.extracted_text {
            metadata.insert("extracted_text".to_string(), text);
        }

        Ok(vec![TextChunk {
            content,
            chunk_index: 0,
            metadata,
        }])
    }
}

impl Default for ImageParser {
    fn default() -> Self {
        Self::new(ImageParserConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_parser_creation() {
        let _parser = ImageParser::default();
        assert!(!ImageParser::supported_formats().is_empty());
    }

    #[test]
    fn test_supported_formats() {
        let formats = ImageParser::supported_formats();
        assert!(formats.contains(&"jpg"));
        assert!(formats.contains(&"png"));
    }
}
