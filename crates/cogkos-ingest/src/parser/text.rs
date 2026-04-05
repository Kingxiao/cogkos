//! Plain text / structured text parser
//!
//! Handles: .txt, .log, .json, .xml, .yaml, .yml, .html, .htm,
//!          .toml, .ini, .cfg, .conf, .properties, .env.example
//!
//! HTML files get script/style/tag stripping before chunking.

use crate::{DocumentParser, TextChunk};
use async_trait::async_trait;
use cogkos_core::{CogKosError, Result};
use regex::Regex;

/// Plain text and structured text parser
pub struct TextParser {
    /// Regex for stripping HTML <script> blocks
    re_script: Regex,
    /// Regex for stripping HTML <style> blocks
    re_style: Regex,
    /// Regex for stripping HTML tags
    re_tags: Regex,
    /// Regex for collapsing multiple blank lines
    re_blank_lines: Regex,
}

impl TextParser {
    pub fn new() -> Self {
        Self {
            re_script: Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap(),
            re_style: Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap(),
            re_tags: Regex::new(r"<[^>]+>").unwrap(),
            re_blank_lines: Regex::new(r"\n{3,}").unwrap(),
        }
    }

    /// Strip HTML tags, scripts, styles — keep text content
    fn strip_html(&self, html: &str) -> String {
        let no_script = self.re_script.replace_all(html, "");
        let no_style = self.re_style.replace_all(&no_script, "");
        let no_tags = self.re_tags.replace_all(&no_style, "");

        // Decode common HTML entities
        let decoded = no_tags
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&nbsp;", " ");

        self.re_blank_lines
            .replace_all(&decoded, "\n\n")
            .to_string()
    }

    /// Check if file is HTML based on extension
    fn is_html(filename: &str) -> bool {
        let lower = filename.to_lowercase();
        lower.ends_with(".html") || lower.ends_with(".htm")
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
        &[
            "txt",
            "log",
            "json",
            "xml",
            "yaml",
            "yml",
            "html",
            "htm",
            "toml",
            "ini",
            "cfg",
            "conf",
            "properties",
        ]
    }

    async fn parse(&self, data: &[u8], filename: &str) -> Result<Vec<TextChunk>> {
        let raw = String::from_utf8(data.to_vec())
            .map_err(|_| CogKosError::Parse("Invalid UTF-8 in text file".to_string()))?;

        let content = if Self::is_html(filename) {
            self.strip_html(&raw)
        } else {
            raw
        };

        let cleaned = super::clean_text(&content);
        if cleaned.trim().is_empty() {
            return Ok(vec![]);
        }

        let file_type = filename.rsplit('.').next().unwrap_or("txt").to_lowercase();

        let chunks = super::chunk_text(&cleaned, 2000, 0);
        let result: Vec<TextChunk> = chunks
            .into_iter()
            .enumerate()
            .filter(|(_, c)| !c.trim().is_empty())
            .map(|(i, c)| {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("source".to_string(), filename.to_string());
                metadata.insert("type".to_string(), file_type.clone());
                TextChunk {
                    content: c.trim().to_string(),
                    chunk_index: i as u32,
                    metadata,
                }
            })
            .collect();

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_plain_text() {
        let parser = TextParser::new();
        let chunks = parser
            .parse(b"Hello world\n\nSecond paragraph", "test.txt")
            .await
            .unwrap();
        assert!(!chunks.is_empty());
        assert!(chunks[0].content.contains("Hello world"));
    }

    #[tokio::test]
    async fn test_json_file() {
        let json = r#"{"key": "value", "nested": {"a": 1}}"#;
        let parser = TextParser::new();
        let chunks = parser.parse(json.as_bytes(), "data.json").await.unwrap();
        assert!(!chunks.is_empty());
        assert!(chunks[0].content.contains("key"));
    }

    #[tokio::test]
    async fn test_html_stripping() {
        let html = r#"<html><head><script>alert('xss')</script><style>body{color:red}</style></head>
<body><h1>Title</h1><p>Hello &amp; world</p></body></html>"#;
        let parser = TextParser::new();
        let chunks = parser.parse(html.as_bytes(), "page.html").await.unwrap();
        assert!(!chunks.is_empty());
        let text = &chunks[0].content;
        assert!(text.contains("Title"), "Missing title text");
        assert!(text.contains("Hello & world"), "Missing decoded entity");
        assert!(!text.contains("<script>"), "Script tag not stripped");
        assert!(!text.contains("alert"), "Script content not stripped");
        assert!(!text.contains("<style>"), "Style tag not stripped");
        assert!(!text.contains("<h1>"), "HTML tag not stripped");
    }

    #[tokio::test]
    async fn test_yaml_file() {
        let yaml = "key: value\nlist:\n  - item1\n  - item2\n";
        let parser = TextParser::new();
        let chunks = parser.parse(yaml.as_bytes(), "config.yaml").await.unwrap();
        assert!(!chunks.is_empty());
        assert!(chunks[0].content.contains("key: value"));
    }

    #[tokio::test]
    async fn test_xml_file() {
        let xml = r#"<?xml version="1.0"?><root><item>Hello</item></root>"#;
        let parser = TextParser::new();
        let chunks = parser.parse(xml.as_bytes(), "data.xml").await.unwrap();
        assert!(!chunks.is_empty());
    }

    #[tokio::test]
    async fn test_empty_file() {
        let parser = TextParser::new();
        let chunks = parser.parse(b"", "empty.txt").await.unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_supported_extensions() {
        let parser = TextParser::new();
        let exts = parser.supported_extensions();
        assert!(exts.contains(&"txt"));
        assert!(exts.contains(&"json"));
        assert!(exts.contains(&"xml"));
        assert!(exts.contains(&"yaml"));
        assert!(exts.contains(&"yml"));
        assert!(exts.contains(&"html"));
        assert!(exts.contains(&"htm"));
        assert!(exts.contains(&"toml"));
        assert!(exts.contains(&"ini"));
        assert!(exts.contains(&"log"));
    }

    #[test]
    fn test_strip_html() {
        let parser = TextParser::new();
        let result = parser.strip_html("<p>Hello &amp; <b>world</b></p>");
        assert_eq!(result.trim(), "Hello & world");
    }
}
