//! Plain text parser implementation

use crate::{DocumentParser, TextChunk};
use async_trait::async_trait;
use cogkos_core::{CogKosError, Result};

/// Plain text parser
pub struct TextParser;

impl TextParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TextParser {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DocumentParser for TextParser {
    fn supported_extensions(&self) -> &[&str] {
        &["txt"]
    }

    async fn parse(&self, data: &[u8], _filename: &str) -> Result<Vec<TextChunk>> {
        let content = String::from_utf8(data.to_vec())
            .map_err(|_| CogKosError::Parse("Invalid UTF-8 in text file".to_string()))?;

        let cleaned = super::clean_text(&content);
        let mut chunks = Vec::new();
        let mut current_chunk = String::new();
        let mut chunk_index = 0;
        let max_chunk_size = 2000;

        // Split into chunks by paragraphs
        for paragraph in cleaned.split("\n\n") {
            let para = paragraph.trim();
            if para.is_empty() {
                continue;
            }

            if current_chunk.len() + para.len() > max_chunk_size && !current_chunk.is_empty() {
                chunks.push(TextChunk {
                    content: current_chunk.trim().to_string(),
                    chunk_index,
                    metadata: std::collections::HashMap::new(),
                });
                chunk_index += 1;
                current_chunk.clear();
            }

            current_chunk.push_str(para);
            current_chunk.push('\n');
        }

        // Add remaining text
        if !current_chunk.trim().is_empty() {
            chunks.push(TextChunk {
                content: current_chunk.trim().to_string(),
                chunk_index,
                metadata: std::collections::HashMap::new(),
            });
        }

        // If no chunks were created
        if chunks.is_empty() && !cleaned.trim().is_empty() {
            chunks.push(TextChunk {
                content: cleaned.trim().to_string(),
                chunk_index: 0,
                metadata: std::collections::HashMap::new(),
            });
        }

        Ok(chunks)
    }
}
