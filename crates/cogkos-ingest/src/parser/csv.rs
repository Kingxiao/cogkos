//! CSV/TSV parser — splits tabular data into chunks with header preservation

use crate::{DocumentParser, TextChunk};
use async_trait::async_trait;
use cogkos_core::{CogKosError, Result};

/// Default number of data rows per chunk (header excluded from count)
const ROWS_PER_CHUNK: usize = 50;

/// CSV / TSV parser
pub struct CsvParser;

impl CsvParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CsvParser {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DocumentParser for CsvParser {
    fn supported_extensions(&self) -> &[&str] {
        &["csv", "tsv"]
    }

    async fn parse(&self, data: &[u8], filename: &str) -> Result<Vec<TextChunk>> {
        let content = String::from_utf8(data.to_vec())
            .map_err(|_| CogKosError::Parse("Invalid UTF-8 in CSV file".to_string()))?;

        let is_tsv = filename.to_lowercase().ends_with(".tsv");
        let delimiter = if is_tsv { '\t' } else { ',' };
        let file_type = if is_tsv { "tsv" } else { "csv" };

        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return Ok(vec![]);
        }

        // First non-empty line is the header
        let header = lines[0];
        let data_lines = &lines[1..];

        if data_lines.is_empty() {
            // Only header, return it as a single chunk
            return Ok(vec![TextChunk {
                content: header.to_string(),
                chunk_index: 0,
                metadata: build_meta(filename, file_type, 0),
            }]);
        }

        let mut chunks = Vec::new();
        let mut chunk_index: u32 = 0;

        for batch in data_lines.chunks(ROWS_PER_CHUNK) {
            let mut text = String::with_capacity(header.len() + batch.len() * 80);
            text.push_str(header);
            text.push('\n');

            for line in batch {
                if line.trim().is_empty() {
                    continue;
                }
                text.push_str(line);
                text.push('\n');
            }

            let trimmed = text.trim().to_string();
            if !trimmed.is_empty() {
                chunks.push(TextChunk {
                    content: format_tabular(&trimmed, delimiter),
                    chunk_index,
                    metadata: build_meta(filename, file_type, chunk_index),
                });
                chunk_index += 1;
            }
        }

        if chunks.is_empty() {
            chunks.push(TextChunk {
                content: header.to_string(),
                chunk_index: 0,
                metadata: build_meta(filename, file_type, 0),
            });
        }

        Ok(chunks)
    }
}

/// Convert raw CSV/TSV text to a more readable column-aligned format.
/// For small tables this helps embedding quality; for large ones it's fine as-is.
fn format_tabular(text: &str, delimiter: char) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= 1 {
        return text.to_string();
    }

    // Parse header columns
    let headers: Vec<&str> = lines[0].split(delimiter).collect();
    let mut output = String::with_capacity(text.len());

    // Keep header line as-is
    output.push_str(lines[0]);
    output.push('\n');

    // Format each data row as "ColName: Value | ColName: Value"
    for line in &lines[1..] {
        let values: Vec<&str> = line.split(delimiter).collect();
        let formatted: Vec<String> = headers
            .iter()
            .zip(values.iter().chain(std::iter::repeat(&"")))
            .filter(|(_, v)| !v.trim().is_empty())
            .map(|(h, v)| format!("{}: {}", h.trim(), v.trim()))
            .collect();

        if !formatted.is_empty() {
            output.push_str(&formatted.join(" | "));
            output.push('\n');
        }
    }

    output.trim().to_string()
}

fn build_meta(
    filename: &str,
    file_type: &str,
    chunk_index: u32,
) -> std::collections::HashMap<String, String> {
    let mut meta = std::collections::HashMap::new();
    meta.insert("source".to_string(), filename.to_string());
    meta.insert("type".to_string(), file_type.to_string());
    meta.insert("chunk_index".to_string(), chunk_index.to_string());
    meta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_csv_basic() {
        let csv = "name,age,city\nAlice,30,Shanghai\nBob,25,Beijing\n";
        let parser = CsvParser::new();
        let chunks = parser.parse(csv.as_bytes(), "test.csv").await.unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("name"));
        assert!(chunks[0].content.contains("Alice"));
        assert!(chunks[0].content.contains("Bob"));
    }

    #[tokio::test]
    async fn test_csv_multiple_chunks() {
        let mut csv = String::from("id,value\n");
        for i in 0..120 {
            csv.push_str(&format!("{},{}\n", i, i * 10));
        }
        let parser = CsvParser::new();
        let chunks = parser.parse(csv.as_bytes(), "big.csv").await.unwrap();
        assert!(
            chunks.len() >= 2,
            "Expected multiple chunks, got {}",
            chunks.len()
        );
        // Each chunk should start with the header
        for chunk in &chunks {
            assert!(chunk.content.contains("id"), "Chunk missing header");
        }
    }

    #[tokio::test]
    async fn test_tsv() {
        let tsv = "name\tage\nAlice\t30\n";
        let parser = CsvParser::new();
        let chunks = parser.parse(tsv.as_bytes(), "data.tsv").await.unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("Alice"));
    }

    #[tokio::test]
    async fn test_csv_empty() {
        let parser = CsvParser::new();
        let chunks = parser.parse(b"", "empty.csv").await.unwrap();
        assert!(chunks.is_empty());
    }

    #[tokio::test]
    async fn test_csv_header_only() {
        let parser = CsvParser::new();
        let chunks = parser.parse(b"col1,col2,col3", "header.csv").await.unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("col1"));
    }

    #[test]
    fn test_supported_extensions() {
        let parser = CsvParser::new();
        assert_eq!(parser.supported_extensions(), &["csv", "tsv"]);
    }
}
