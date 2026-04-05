//! Excel parser implementation

use crate::{DocumentParser, TextChunk};
use async_trait::async_trait;
use calamine::{Data, Reader, Xlsx};
use cogkos_core::{CogKosError, Result};
use std::io::Cursor;

/// Excel parser
pub struct ExcelParser;

impl ExcelParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ExcelParser {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DocumentParser for ExcelParser {
    fn supported_extensions(&self) -> &[&str] {
        &["xlsx", "xls"]
    }

    async fn parse(&self, data: &[u8], filename: &str) -> Result<Vec<TextChunk>> {
        let data_vec = data.to_vec();
        let filename = filename.to_string();

        let chunks = tokio::task::spawn_blocking(move || -> Result<Vec<TextChunk>> {
            // Open workbook from cursor using Reader trait
            let cursor = Cursor::new(data_vec);
            let mut workbook: Xlsx<Cursor<Vec<u8>>> = Xlsx::new(cursor)
                .map_err(|e| CogKosError::Parse(format!("Excel open error: {:?}", e)))?;

            let sheet_names = workbook.sheet_names().to_vec();
            let mut chunks = Vec::new();
            let mut chunk_index = 0;
            let max_chunk_size = 2000;
            let mut current_chunk = String::new();

            for sheet_name in sheet_names {
                let range = workbook
                    .worksheet_range(&sheet_name)
                    .map_err(|e| CogKosError::Parse(format!("Excel sheet error: {:?}", e)))?;

                // Add sheet header
                current_chunk.push_str(&format!("=== Sheet: {} ===\n", sheet_name));

                // Get rows
                let rows: Vec<Vec<String>> = range
                    .rows()
                    .map(|row| {
                        row.iter()
                            .map(|cell| match cell {
                                Data::Int(i) => i.to_string(),
                                Data::Float(f) => f.to_string(),
                                Data::String(s) => s.clone(),
                                Data::Bool(b) => b.to_string(),
                                Data::DateTime(dt) => dt.to_string(),
                                Data::DateTimeIso(s) => s.clone(),
                                Data::DurationIso(s) => s.clone(),
                                Data::Error(e) => format!("Error: {:?}", e),
                                Data::Empty => String::new(),
                            })
                            .collect()
                    })
                    .collect();

                // Process rows into chunks
                for row in rows {
                    let row_text = row
                        .iter()
                        .filter(|s| !s.is_empty())
                        .enumerate()
                        .map(|(i, s)| {
                            // Simple column reference (A, B, C, ...)
                            let col_letter = (b'A' + i as u8) as char;
                            format!("{}: {}", col_letter, s)
                        })
                        .collect::<Vec<_>>()
                        .join(" | ");

                    if row_text.is_empty() {
                        continue;
                    }

                    if current_chunk.len() + row_text.len() > max_chunk_size
                        && !current_chunk.is_empty()
                    {
                        chunks.push(TextChunk {
                            content: current_chunk.trim().to_string(),
                            chunk_index,
                            metadata: {
                                let mut meta = std::collections::HashMap::new();
                                meta.insert("source".to_string(), filename.clone());
                                meta.insert("type".to_string(), "excel".to_string());
                                meta.insert("sheet".to_string(), sheet_name.clone());
                                meta
                            },
                        });
                        chunk_index += 1;
                        current_chunk.clear();
                    }

                    current_chunk.push_str(&row_text);
                    current_chunk.push('\n');
                }

                current_chunk.push('\n');
            }

            // Add remaining content
            if !current_chunk.trim().is_empty() {
                chunks.push(TextChunk {
                    content: current_chunk.trim().to_string(),
                    chunk_index,
                    metadata: {
                        let mut meta = std::collections::HashMap::new();
                        meta.insert("source".to_string(), filename.clone());
                        meta.insert("type".to_string(), "excel".to_string());
                        meta
                    },
                });
            }

            // If no chunks were created
            if chunks.is_empty() {
                chunks.push(TextChunk {
                    content: format!("Excel file: {}", filename),
                    chunk_index: 0,
                    metadata: {
                        let mut meta = std::collections::HashMap::new();
                        meta.insert("source".to_string(), filename.clone());
                        meta.insert("type".to_string(), "excel".to_string());
                        meta
                    },
                });
            }

            Ok(chunks)
        })
        .await
        .map_err(|e| CogKosError::Internal(format!("Join error: {}", e)))??;

        Ok(chunks)
    }
}
