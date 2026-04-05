//! Document parsers for various formats

use crate::image::ImageParser;
use crate::{DocumentParser, TextChunk};
use cogkos_core::{CogKosError, Result};

mod audio;
mod csv;
mod excel;
mod markdown;
mod pdf;
mod powerpoint;
mod text;
mod video;
mod word;

pub use audio::{AudioParser, AudioParserConfig, AudioTranscription};
pub use csv::CsvParser;
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
        registry.register(Box::new(CsvParser::new()));
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

/// Minimum segment size before merging with neighbors
const MIN_SEGMENT_SIZE: usize = 200;

/// Chunk text into smaller pieces using semantic boundaries.
///
/// Strategy (priority order):
/// 1. Split by `\n\n` (paragraph boundaries)
/// 2. If a paragraph > max_chunk_size, split by `\n` (line boundaries)
/// 3. If a single line > max_chunk_size, split by character window
/// 4. Merge short consecutive segments until approaching max_chunk_size
/// 5. Apply overlap between final chunks
pub fn chunk_text(text: &str, max_chunk_size: usize, overlap: usize) -> Vec<String> {
    if text.len() <= max_chunk_size {
        return vec![text.to_string()];
    }

    // Phase 1: Split into atomic segments respecting semantic boundaries
    let segments = split_into_segments(text, max_chunk_size);

    // Phase 2: Merge short segments up to max_chunk_size
    let merged = merge_short_segments(&segments, max_chunk_size);

    // Phase 3: Apply overlap between chunks
    if overlap == 0 || merged.len() <= 1 {
        return merged;
    }
    apply_overlap(&merged, overlap)
}

/// Split text into atomic segments: paragraphs -> lines -> char windows
fn split_into_segments(text: &str, max_size: usize) -> Vec<String> {
    let mut segments = Vec::new();

    for paragraph in text.split("\n\n") {
        let trimmed = paragraph.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.len() <= max_size {
            segments.push(trimmed.to_string());
        } else {
            // Paragraph too large — split by lines
            for line in trimmed.split('\n') {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if line.len() <= max_size {
                    segments.push(line.to_string());
                } else {
                    // Line too large — character window fallback
                    segments.extend(split_by_chars(line, max_size));
                }
            }
        }
    }

    segments
}

/// Character-level splitting as last resort
fn split_by_chars(text: &str, max_size: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();

    while start < bytes.len() {
        let mut end = (start + max_size).min(bytes.len());
        // Avoid splitting in the middle of a UTF-8 character
        while end < bytes.len() && !text.is_char_boundary(end) {
            end -= 1;
        }
        // Try to break at a space for readability
        if end < bytes.len() {
            if let Some(space_pos) = text[start..end].rfind(' ') {
                if space_pos > max_size / 2 {
                    end = start + space_pos;
                }
            }
        }
        chunks.push(text[start..end].to_string());
        start = end;
    }

    chunks
}

/// Merge consecutive short segments until they approach max_chunk_size
fn merge_short_segments(segments: &[String], max_size: usize) -> Vec<String> {
    let mut merged = Vec::new();
    let mut buffer = String::new();

    for segment in segments {
        if buffer.is_empty() {
            buffer = segment.clone();
            continue;
        }

        // Would merging exceed max_size?
        let combined_len = buffer.len() + 2 + segment.len(); // +2 for "\n\n" separator
        if combined_len <= max_size && buffer.len() < MIN_SEGMENT_SIZE {
            buffer.push_str("\n\n");
            buffer.push_str(segment);
        } else if combined_len <= max_size {
            // Current buffer is already large enough to stand alone,
            // but we can still merge if the segment is short
            if segment.len() < MIN_SEGMENT_SIZE {
                buffer.push_str("\n\n");
                buffer.push_str(segment);
            } else {
                merged.push(std::mem::take(&mut buffer));
                buffer = segment.clone();
            }
        } else {
            merged.push(std::mem::take(&mut buffer));
            buffer = segment.clone();
        }
    }

    if !buffer.is_empty() {
        merged.push(buffer);
    }

    merged
}

/// Apply overlap: prepend tail of previous chunk to current chunk
fn apply_overlap(chunks: &[String], overlap: usize) -> Vec<String> {
    let mut result = Vec::with_capacity(chunks.len());
    result.push(chunks[0].clone());

    for i in 1..chunks.len() {
        let prev = &chunks[i - 1];
        let tail_start = if prev.len() > overlap {
            prev.len() - overlap
        } else {
            0
        };
        // Find a clean break point (newline or space)
        let tail = &prev[tail_start..];
        let clean_start = tail
            .find('\n')
            .or_else(|| tail.find(' '))
            .map(|p| p + 1)
            .unwrap_or(0);

        let overlap_text = &tail[clean_start..];
        if overlap_text.is_empty() {
            result.push(chunks[i].clone());
        } else {
            result.push(format!("{}\n\n{}", overlap_text, chunks[i]));
        }
    }

    result
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
