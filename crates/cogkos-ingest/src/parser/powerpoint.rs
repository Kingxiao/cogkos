//! PowerPoint parser — extracts text from PPTX files
//!
//! PPTX is a ZIP archive containing XML slides.
//! Text content lives in `ppt/slides/slideN.xml` within `<a:t>` elements.

use crate::{DocumentParser, TextChunk};
use async_trait::async_trait;
use cogkos_core::Result;
use std::io::Read;

pub struct PptxParser;

impl PptxParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PptxParser {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DocumentParser for PptxParser {
    fn supported_extensions(&self) -> &[&str] {
        &["pptx"]
    }

    async fn parse(&self, data: &[u8], filename: &str) -> Result<Vec<TextChunk>> {
        let cursor = std::io::Cursor::new(data);
        let mut archive = zip::ZipArchive::new(cursor).map_err(|e| {
            cogkos_core::CogKosError::Parse(format!("Invalid PPTX file {}: {}", filename, e))
        })?;

        let mut chunks = Vec::new();
        let mut slide_names: Vec<String> = Vec::new();

        // Collect slide file names (sorted for deterministic order)
        for i in 0..archive.len() {
            if let Ok(file) = archive.by_index(i) {
                let name = file.name().to_string();
                if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
                    slide_names.push(name);
                }
            }
        }
        slide_names.sort();

        for (slide_idx, slide_name) in slide_names.iter().enumerate() {
            let mut xml_content = String::new();
            if let Ok(mut file) = archive.by_name(slide_name) {
                file.read_to_string(&mut xml_content).map_err(|e| {
                    cogkos_core::CogKosError::Parse(format!(
                        "Failed to read {}: {}",
                        slide_name, e
                    ))
                })?;
            } else {
                continue;
            }

            let text = extract_text_from_slide_xml(&xml_content);
            if !text.is_empty() {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("source".to_string(), filename.to_string());
                metadata.insert("slide_number".to_string(), (slide_idx + 1).to_string());
                metadata.insert("type".to_string(), "pptx_slide".to_string());
                chunks.push(TextChunk {
                    content: text,
                    chunk_index: slide_idx as u32,
                    metadata,
                });
            }
        }

        if chunks.is_empty() {
            tracing::warn!("No text content found in PPTX: {}", filename);
        }

        Ok(chunks)
    }
}

/// Extract text from PPTX slide XML by parsing `<a:t>` elements
fn extract_text_from_slide_xml(xml: &str) -> String {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    let mut texts = Vec::new();
    let mut in_text_element = false;
    let mut current_paragraph = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local = e.local_name();
                // <a:t> contains text runs
                if local.as_ref() == b"t" {
                    in_text_element = true;
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_text_element
                    && let Ok(text) = e.unescape() {
                        let t = text.trim().to_string();
                        if !t.is_empty() {
                            current_paragraph.push(t);
                        }
                    }
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name();
                if local.as_ref() == b"t" {
                    in_text_element = false;
                }
                // End of paragraph <a:p> — join runs and push
                if local.as_ref() == b"p" && !current_paragraph.is_empty() {
                    texts.push(current_paragraph.join(""));
                    current_paragraph.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    // Flush any remaining paragraph
    if !current_paragraph.is_empty() {
        texts.push(current_paragraph.join(""));
    }

    texts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text_from_slide_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
       xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:sp>
        <p:txBody>
          <a:p><a:r><a:t>Hello World</a:t></a:r></a:p>
          <a:p><a:r><a:t>Second paragraph</a:t></a:r></a:p>
        </p:txBody>
      </p:sp>
    </p:spTree>
  </p:cSld>
</p:sld>"#;

        let text = extract_text_from_slide_xml(xml);
        assert!(text.contains("Hello World"));
        assert!(text.contains("Second paragraph"));
    }

    #[test]
    fn test_pptx_parser_creation() {
        let parser = PptxParser;
        assert_eq!(parser.supported_extensions(), &["pptx"]);
    }
}
