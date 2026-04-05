//! Word/DOCX parser implementation

use crate::{DocumentParser, TextChunk};
use async_trait::async_trait;
use cogkos_core::{CogKosError, Result};
use docx_rs::{DocumentChild, Docx, ParagraphChild, RunChild};

/// Word/DOCX parser
pub struct DocxParser;

impl DocxParser {
    pub fn new() -> Self {
        Self
    }

    /// Extract text from a DOCX document
    fn extract_text(&self, doc: &Docx) -> String {
        let mut text_parts = Vec::new();

        let document = &doc.document;
        for child in &document.children {
            // Note: Table extraction requires more complex handling
            // For now, we focus on paragraph text extraction
            if let DocumentChild::Paragraph(paragraph) = child {
                let para_text = self.extract_paragraph_text(paragraph);
                if !para_text.is_empty() {
                    text_parts.push(para_text);
                }
            }
        }

        text_parts.join("\n\n")
    }

    /// Extract text from a paragraph
    fn extract_paragraph_text(&self, paragraph: &docx_rs::Paragraph) -> String {
        let mut text_parts = Vec::new();

        for child in &paragraph.children {
            if let ParagraphChild::Run(run) = child {
                let run_text = self.extract_run_text(run);
                if !run_text.is_empty() {
                    text_parts.push(run_text);
                }
            }
        }

        text_parts.join("")
    }

    /// Extract text from a run
    fn extract_run_text(&self, run: &docx_rs::Run) -> String {
        let mut text_parts = Vec::new();

        for child in &run.children {
            match child {
                RunChild::Text(text) => {
                    text_parts.push(text.text.clone());
                }
                RunChild::Break(_) => {
                    // Add space for breaks
                    text_parts.push(" ".to_string());
                }
                _ => {}
            }
        }

        text_parts.join("")
    }
}

impl Default for DocxParser {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DocumentParser for DocxParser {
    fn supported_extensions(&self) -> &[&str] {
        &["docx"]
    }

    async fn parse(&self, data: &[u8], _filename: &str) -> Result<Vec<TextChunk>> {
        // Parse DOCX file
        let doc = docx_rs::read_docx(data)
            .map_err(|e| CogKosError::Parse(format!("DOCX parsing failed: {:?}", e)))?;

        // Extract text content
        let text = self.extract_text(&doc);

        // Clean the extracted text
        let cleaned_text = super::clean_text(&text);

        // Chunk the text
        let max_chunk_size = 2000;
        let mut chunks = Vec::new();
        let mut chunk_index = 0;

        if cleaned_text.len() <= max_chunk_size {
            // Single chunk
            chunks.push(TextChunk {
                content: cleaned_text.clone(),
                chunk_index,
                metadata: std::collections::HashMap::new(),
            });
        } else {
            // Split into multiple chunks
            for chunk in super::chunk_text(&cleaned_text, max_chunk_size, 100) {
                if !chunk.trim().is_empty() {
                    chunks.push(TextChunk {
                        content: chunk,
                        chunk_index,
                        metadata: std::collections::HashMap::new(),
                    });
                    chunk_index += 1;
                }
            }
        }

        // If no chunks were created, treat entire content as one chunk
        if chunks.is_empty() && !cleaned_text.is_empty() {
            chunks.push(TextChunk {
                content: cleaned_text,
                chunk_index: 0,
                metadata: std::collections::HashMap::new(),
            });
        }

        Ok(chunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_extensions() {
        let parser = DocxParser::new();
        let extensions = parser.supported_extensions();
        assert!(extensions.contains(&"docx"));
    }
}
