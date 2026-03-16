//! Markdown parser implementation

use crate::{DocumentParser, TextChunk};
use async_trait::async_trait;
use cogkos_core::{CogKosError, Result};

/// Markdown parser
pub struct MarkdownParser;

impl MarkdownParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MarkdownParser {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DocumentParser for MarkdownParser {
    fn supported_extensions(&self) -> &[&str] {
        &["md", "markdown"]
    }

    async fn parse(&self, data: &[u8], _filename: &str) -> Result<Vec<TextChunk>> {
        let content = String::from_utf8(data.to_vec())
            .map_err(|_| CogKosError::Parse("Invalid UTF-8 in markdown".to_string()))?;

        let mut chunks = Vec::new();
        let mut current_chunk = String::new();
        let mut chunk_index = 0;
        let max_chunk_size = 2000;

        // Parse markdown structure
        let parser = pulldown_cmark::Parser::new(&content);

        for event in parser {
            use pulldown_cmark::Event;
            use pulldown_cmark::Tag;

            match event {
                Event::Text(text) | Event::Code(text) => {
                    current_chunk.push_str(&text);
                }
                Event::End(Tag::Paragraph) | Event::End(Tag::Heading(_, _, _)) => {
                    if !current_chunk.trim().is_empty() {
                        self.push_chunk(
                            &mut chunks,
                            &mut current_chunk,
                            &mut chunk_index,
                            max_chunk_size,
                        );
                    }
                }
                Event::End(Tag::TableCell) => {
                    current_chunk.push_str(" | ");
                }
                Event::End(Tag::TableRow) => {
                    current_chunk.push('\n');
                }
                Event::End(Tag::Table(_)) => {
                    current_chunk.push('\n');
                    self.push_chunk(
                        &mut chunks,
                        &mut current_chunk,
                        &mut chunk_index,
                        max_chunk_size,
                    );
                }
                Event::SoftBreak | Event::HardBreak => {
                    current_chunk.push('\n');
                }
                _ => {}
            }
        }

        // Add remaining text
        if !current_chunk.trim().is_empty() {
            self.push_chunk(
                &mut chunks,
                &mut current_chunk,
                &mut chunk_index,
                max_chunk_size,
            );
        }

        // If no chunks were created, treat entire content as one chunk
        if chunks.is_empty() && !content.trim().is_empty() {
            chunks.push(TextChunk {
                content: content.trim().to_string(),
                chunk_index: 0,
                metadata: std::collections::HashMap::new(),
            });
        }

        Ok(chunks)
    }
}

impl MarkdownParser {
    fn push_chunk(
        &self,
        chunks: &mut Vec<TextChunk>,
        current_chunk: &mut String,
        chunk_index: &mut u32,
        max_chunk_size: usize,
    ) {
        let text = current_chunk.trim();
        if text.is_empty() {
            return;
        }

        if text.len() > max_chunk_size {
            // Split large chunks
            for chunk in crate::parser::chunk_text(text, max_chunk_size, 100) {
                chunks.push(TextChunk {
                    content: chunk,
                    chunk_index: *chunk_index,
                    metadata: std::collections::HashMap::new(),
                });
                *chunk_index += 1;
            }
        } else {
            chunks.push(TextChunk {
                content: text.to_string(),
                chunk_index: *chunk_index,
                metadata: std::collections::HashMap::new(),
            });
            *chunk_index += 1;
        }
        current_chunk.clear();
    }
}
