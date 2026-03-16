//! Document parsers for various formats

use crate::image::ImageParser;
use crate::{DocumentParser, TextChunk};
use cogkos_core::{CogKosError, Result};

mod audio;
mod excel;
mod markdown;
mod pdf;
mod powerpoint;
mod text;
mod video;
mod word;

pub use audio::{AudioParser, AudioParserConfig, AudioTranscription};
pub use excel::ExcelParser;
pub use markdown::MarkdownParser;
pub use pdf::PdfParser;
pub use powerpoint::PptxParser;
pub use text::TextParser;
pub use video::VideoParser;
pub use word::DocxParser;

/// Parser registry for managing multiple parsers
pub struct ParserRegistry {
    parsers: Vec<Box<dyn DocumentParser>>,
}

impl ParserRegistry {
    /// Create new registry with default parsers
    pub fn new() -> Self {
        let mut registry = Self {
            parsers: Vec::new(),
        };
        registry.register(Box::new(MarkdownParser::new()));
        registry.register(Box::new(PdfParser::new()));
        registry.register(Box::new(DocxParser::new()));
        registry.register(Box::new(ExcelParser::new()));
        registry.register(Box::new(PptxParser::new()));
        registry.register(Box::new(TextParser::new()));
        registry.register(Box::new(ImageParser::default()));
        registry.register(Box::new(AudioParser::default()));
        registry.register(Box::new(VideoParser::default()));
        registry
    }

    /// Register a parser
    pub fn register(&mut self, parser: Box<dyn DocumentParser>) {
        self.parsers.push(parser);
    }

    /// Get parser for filename
    pub fn get_parser(&self, filename: &str) -> Option<&dyn DocumentParser> {
        let extension = filename.split('.').next_back()?.to_lowercase();
        self.parsers
            .iter()
            .find(|p| p.supported_extensions().contains(&extension.as_str()))
            .map(|p| p.as_ref())
    }

    /// Parse file using appropriate parser
    pub async fn parse(&self, data: &[u8], filename: &str) -> Result<Vec<TextChunk>> {
        match self.get_parser(filename) {
            Some(parser) => parser.parse(data, filename).await,
            None => Err(CogKosError::Parse(format!(
                "No parser found for file: {}",
                filename
            ))),
        }
    }
}

impl Default for ParserRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Chunk text into smaller pieces
pub fn chunk_text(text: &str, max_chunk_size: usize, overlap: usize) -> Vec<String> {
    if text.len() <= max_chunk_size {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let end = (start + max_chunk_size).min(text.len());
        let chunk = &text[start..end];
        chunks.push(chunk.to_string());

        // Move start forward with overlap
        start += max_chunk_size - overlap;

        // Avoid infinite loop on very small progress
        if start >= end {
            break;
        }
    }

    chunks
}

/// Clean extracted text
pub fn clean_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace("\r", "\n")
        .split('\n')
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
