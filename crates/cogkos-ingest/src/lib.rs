//! CogKOS Ingest - Document ingestion pipeline

use async_trait::async_trait;
use cogkos_core::Result;
use cogkos_core::models::*;
use uuid::Uuid;

pub mod classifier;
pub mod coordinator;
pub mod deep_classifier;
pub mod embedding;
pub mod extractor;
pub mod image;
pub mod parser;
pub mod pipeline;

pub use classifier::*;
pub use coordinator::*;
pub use deep_classifier::*;
pub use embedding::*;
pub use extractor::*;
pub use image::*;
pub use parser::*;
pub use pipeline::*;

/// Text chunk from document parsing
#[derive(Debug, Clone)]
pub struct TextChunk {
    pub content: String,
    pub chunk_index: u32,
    pub metadata: std::collections::HashMap<String, String>,
}

/// Document type detected from content
#[derive(Debug, Clone)]
pub enum DocumentType {
    Pdf,
    Word,
    Markdown,
    Text,
    Excel,
    PowerPoint,
    Csv,
    Unknown(String),
}

impl DocumentType {
    /// Detect from filename extension
    pub fn from_filename(filename: &str) -> Self {
        let lower = filename.to_lowercase();
        if lower.ends_with(".pdf") {
            DocumentType::Pdf
        } else if lower.ends_with(".docx") || lower.ends_with(".doc") {
            DocumentType::Word
        } else if lower.ends_with(".md") || lower.ends_with(".markdown") {
            DocumentType::Markdown
        } else if lower.ends_with(".csv") || lower.ends_with(".tsv") {
            DocumentType::Csv
        } else if lower.ends_with(".txt")
            || lower.ends_with(".log")
            || lower.ends_with(".json")
            || lower.ends_with(".xml")
            || lower.ends_with(".yaml")
            || lower.ends_with(".yml")
            || lower.ends_with(".html")
            || lower.ends_with(".htm")
            || lower.ends_with(".toml")
            || lower.ends_with(".ini")
            || lower.ends_with(".cfg")
            || lower.ends_with(".conf")
            || lower.ends_with(".properties")
        {
            DocumentType::Text
        } else if lower.ends_with(".xlsx") || lower.ends_with(".xls") {
            DocumentType::Excel
        } else if lower.ends_with(".pptx") || lower.ends_with(".ppt") {
            DocumentType::PowerPoint
        } else {
            DocumentType::Unknown(lower.split('.').next_back().unwrap_or("").to_string())
        }
    }

    /// Get MIME type
    pub fn mime_type(&self) -> &'static str {
        match self {
            DocumentType::Pdf => "application/pdf",
            DocumentType::Word => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            }
            DocumentType::Markdown => "text/markdown",
            DocumentType::Text => "text/plain",
            DocumentType::Excel => {
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            }
            DocumentType::PowerPoint => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
            DocumentType::Csv => "text/csv",
            DocumentType::Unknown(_) => "application/octet-stream",
        }
    }
}

/// Uploaded file for ingestion
#[derive(Debug, Clone)]
pub struct UploadedFile {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
    pub source: Claimant,
    pub tenant_id: String,
}

/// Ingestion result
#[derive(Debug, Clone)]
pub struct IngestResult {
    pub file_claim_id: Uuid,
    pub chunk_claim_ids: Vec<Uuid>,
    pub conflicts_detected: Vec<ConflictRecord>,
    pub novelty_score: f64,
    /// Deep classification results (Phase 3)
    pub deep_classification: Option<deep_classifier::DeepClassification>,
}

/// Document parser trait
#[async_trait]
pub trait DocumentParser: Send + Sync {
    /// Get supported extensions
    fn supported_extensions(&self) -> &[&str];

    /// Parse document into text chunks
    async fn parse(&self, data: &[u8], filename: &str) -> Result<Vec<TextChunk>>;
}
