//! PDF parser implementation

use crate::{DocumentParser, TextChunk};
use async_trait::async_trait;
use cogkos_core::{CogKosError, Result};

use tracing::debug;
/// PDF parser with configurable chunk size
pub struct PdfParser {
    max_chunk_size: usize,
}

impl PdfParser {
    pub fn new() -> Self {
        Self {
            max_chunk_size: 2000,
        }
    }

    /// Create a new PdfParser with custom max chunk size
    pub fn with_max_chunk_size(size: usize) -> Self {
        Self {
            max_chunk_size: size,
        }
    }
}

impl Default for PdfParser {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DocumentParser for PdfParser {
    fn supported_extensions(&self) -> &[&str] {
        &["pdf"]
    }

    async fn parse(&self, data: &[u8], _filename: &str) -> Result<Vec<TextChunk>> {
        // Use pdf-extract to extract text
        // pdf-extract is sync, so we wrap it or just call it if it's not blocking for too long
        // For larger files, we might want to use tokio::task::spawn_blocking
        let data_vec = data.to_vec();
        let text =
            tokio::task::spawn_blocking(move || pdf_extract::extract_text_from_mem(&data_vec))
                .await
                .map_err(|e| CogKosError::Internal(format!("Join error: {}", e)))?
                .map_err(|e| CogKosError::Parse(format!("PDF extraction failed: {}", e)))?;

        let cleaned = super::clean_text(&text);
        debug!(
            "Extracted {} characters from PDF, cleaned to {}",
            text.len(),
            cleaned.len()
        );

        // Split into chunks by paragraphs with size limit
        let mut chunks = Vec::new();
        let mut current_chunk = String::new();
        let mut chunk_index = 0;
        let max_chunk_size = self.max_chunk_size;

        for paragraph in cleaned.split("\n\n") {
            let para = paragraph.trim();
            if para.is_empty() {
                continue;
            }

            // If single paragraph exceeds max size, split it further
            if para.len() > max_chunk_size {
                // First, save current chunk if not empty
                if !current_chunk.is_empty() {
                    chunks.push(TextChunk {
                        content: current_chunk.trim().to_string(),
                        chunk_index,
                        metadata: std::collections::HashMap::new(),
                    });
                    chunk_index += 1;
                    current_chunk.clear();
                }

                // Split large paragraph into smaller pieces
                let words: Vec<&str> = para.split_whitespace().collect();
                let mut temp_chunk = String::new();

                for word in words {
                    if temp_chunk.len() + word.len() + 1 > max_chunk_size {
                        chunks.push(TextChunk {
                            content: temp_chunk.trim().to_string(),
                            chunk_index,
                            metadata: std::collections::HashMap::new(),
                        });
                        chunk_index += 1;
                        temp_chunk.clear();
                    }
                    if !temp_chunk.is_empty() {
                        temp_chunk.push(' ');
                    }
                    temp_chunk.push_str(word);
                }

                // Add remaining words
                if !temp_chunk.trim().is_empty() {
                    current_chunk = temp_chunk;
                }
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

        // If no chunks were created, treat entire content as one chunk
        if chunks.is_empty() && !cleaned.trim().is_empty() {
            chunks.push(TextChunk {
                content: cleaned.trim().to_string(),
                chunk_index: 0,
                metadata: std::collections::HashMap::new(),
            });
        }

        debug!("Created {} chunks from PDF", chunks.len());
        Ok(chunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdf_parser_default() {
        let parser = PdfParser::new();
        assert_eq!(parser.supported_extensions(), &["pdf"]);
    }

    #[test]
    fn test_pdf_parser_custom_chunk_size() {
        let parser = PdfParser::with_max_chunk_size(1000);
        assert_eq!(parser.max_chunk_size, 1000);
    }

    #[tokio::test]
    async fn test_pdf_parser_empty_data() {
        let parser = PdfParser::new();
        let result = parser.parse(&[], "test.pdf").await;
        assert!(result.is_err());
    }
}
