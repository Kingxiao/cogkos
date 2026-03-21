//! LLM-based knowledge extraction from document chunks.
//!
//! When an LLM client is available, each text chunk is analyzed to extract
//! structured knowledge (facts, decisions, predictions, relations).
//! Falls back gracefully to raw-text storage when LLM is unavailable.

use cogkos_core::models::NodeType;
use cogkos_llm::client::LlmClient;
use cogkos_llm::types::{LlmRequest, Message, Role};
use serde::Deserialize;
use std::sync::Arc;
use tracing::{debug, warn};

/// Default confidence for LLM-extracted knowledge (not full trust)
const DEFAULT_EXTRACTION_CONFIDENCE: f64 = 0.7;

/// Extraction prompt — instructs LLM to return structured JSON
const EXTRACTION_PROMPT: &str = r#"You are a knowledge extraction engine. Analyze the text and extract structured knowledge.

Return ONLY valid JSON (no markdown fences) with this schema:
{
  "facts": [{"content": "...", "confidence": 0.0-1.0}],
  "decisions": [{"content": "...", "confidence": 0.0-1.0}],
  "predictions": [{"content": "...", "confidence": 0.0-1.0}],
  "relations": [{"subject": "...", "relation": "...", "object": "..."}]
}

Rules:
- facts: concrete, verifiable statements (entities, attributes, measurements)
- decisions: conclusions, design choices, judgments made in the text
- predictions: forecasts, expectations, hypotheses about the future
- relations: semantic relationships between concepts (e.g. "Rust" -uses-> "ownership model")
- confidence: your estimate of factual reliability (0.0 = uncertain, 1.0 = definitive)
- If a category has no items, use an empty array []
- Extract only what is explicitly stated or strongly implied — do not hallucinate
- Keep each content string concise (1-2 sentences max)"#;

/// Extracts structured knowledge from text chunks via LLM
pub struct KnowledgeExtractor {
    llm_client: Arc<dyn LlmClient>,
}

/// A single extracted knowledge item
#[derive(Debug, Clone)]
pub struct ExtractedItem {
    pub content: String,
    pub confidence: f64,
    pub node_type: NodeType,
}

/// A subject-relation-object triple
#[derive(Debug, Clone)]
pub struct ExtractedRelation {
    pub subject: String,
    pub relation: String,
    pub object: String,
}

/// Full extraction result from a single chunk
#[derive(Debug, Clone)]
pub struct ExtractedKnowledge {
    pub facts: Vec<ExtractedItem>,
    pub decisions: Vec<ExtractedItem>,
    pub predictions: Vec<ExtractedItem>,
    pub relations: Vec<ExtractedRelation>,
}

/// Raw JSON deserialization target
#[derive(Debug, Deserialize)]
struct RawExtraction {
    #[serde(default)]
    facts: Vec<RawItem>,
    #[serde(default)]
    decisions: Vec<RawItem>,
    #[serde(default)]
    predictions: Vec<RawItem>,
    #[serde(default)]
    relations: Vec<RawRelation>,
}

#[derive(Debug, Deserialize)]
struct RawItem {
    content: String,
    #[serde(default = "default_confidence")]
    confidence: f64,
}

#[derive(Debug, Deserialize)]
struct RawRelation {
    subject: String,
    relation: String,
    object: String,
}

fn default_confidence() -> f64 {
    DEFAULT_EXTRACTION_CONFIDENCE
}

impl KnowledgeExtractor {
    pub fn new(llm_client: Arc<dyn LlmClient>) -> Self {
        Self { llm_client }
    }

    /// Extract structured knowledge from a text chunk.
    ///
    /// `domain` provides context to the LLM about the subject area.
    /// Returns `None` if extraction fails (caller should fall back to raw text).
    pub async fn extract(&self, chunk_text: &str, domain: &str) -> Option<ExtractedKnowledge> {
        if chunk_text.trim().len() < 50 {
            debug!(
                "Chunk too short for extraction ({} chars), skipping",
                chunk_text.len()
            );
            return None;
        }

        let user_msg = if domain.is_empty() || domain == "unclassified" {
            format!(
                "Extract knowledge from the following text:\n\n{}",
                chunk_text
            )
        } else {
            format!(
                "Domain: {}\n\nExtract knowledge from the following text:\n\n{}",
                domain, chunk_text
            )
        };

        let request = LlmRequest {
            messages: vec![
                Message {
                    role: Role::System,
                    content: EXTRACTION_PROMPT.to_string(),
                },
                Message {
                    role: Role::User,
                    content: user_msg,
                },
            ],
            max_tokens: Some(2000),
            temperature: 0.1, // Low temperature for structured extraction
            ..Default::default()
        };

        let response = match self.llm_client.chat(request).await {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "LLM extraction failed, falling back to raw text");
                return None;
            }
        };

        parse_extraction_response(&response.content)
    }
}

/// Parse LLM response JSON into ExtractedKnowledge
fn parse_extraction_response(content: &str) -> Option<ExtractedKnowledge> {
    // Strip markdown code fences if present
    let json_str = content
        .trim()
        .strip_prefix("```json")
        .or_else(|| content.trim().strip_prefix("```"))
        .unwrap_or(content.trim());
    let json_str = json_str.strip_suffix("```").unwrap_or(json_str).trim();

    let raw: RawExtraction = match serde_json::from_str(json_str) {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "Failed to parse LLM extraction JSON");
            debug!(raw_content = %content, "Raw LLM response that failed to parse");
            return None;
        }
    };

    let clamp = |c: f64| c.clamp(0.0, DEFAULT_EXTRACTION_CONFIDENCE);

    let facts = raw
        .facts
        .into_iter()
        .filter(|i| !i.content.trim().is_empty())
        .map(|i| ExtractedItem {
            content: i.content,
            confidence: clamp(i.confidence),
            node_type: NodeType::Entity,
        })
        .collect();

    let decisions = raw
        .decisions
        .into_iter()
        .filter(|i| !i.content.trim().is_empty())
        .map(|i| ExtractedItem {
            content: i.content,
            confidence: clamp(i.confidence),
            node_type: NodeType::Insight,
        })
        .collect();

    let predictions = raw
        .predictions
        .into_iter()
        .filter(|i| !i.content.trim().is_empty())
        .map(|i| ExtractedItem {
            content: i.content,
            confidence: clamp(i.confidence),
            node_type: NodeType::Prediction,
        })
        .collect();

    let relations = raw
        .relations
        .into_iter()
        .filter(|r| {
            !r.subject.trim().is_empty()
                && !r.relation.trim().is_empty()
                && !r.object.trim().is_empty()
        })
        .map(|r| ExtractedRelation {
            subject: r.subject,
            relation: r.relation,
            object: r.object,
        })
        .collect();

    Some(ExtractedKnowledge {
        facts,
        decisions,
        predictions,
        relations,
    })
}

impl ExtractedKnowledge {
    /// Total number of extracted items (excluding relations)
    pub fn item_count(&self) -> usize {
        self.facts.len() + self.decisions.len() + self.predictions.len()
    }

    /// Check if extraction yielded any items
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
            && self.decisions.is_empty()
            && self.predictions.is_empty()
            && self.relations.is_empty()
    }

    /// Flatten all items into a single iterator
    pub fn all_items(&self) -> impl Iterator<Item = &ExtractedItem> {
        self.facts
            .iter()
            .chain(self.decisions.iter())
            .chain(self.predictions.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_json() {
        let json = r#"{
            "facts": [{"content": "Rust has no GC", "confidence": 0.95}],
            "decisions": [{"content": "Use ownership model", "confidence": 0.8}],
            "predictions": [],
            "relations": [{"subject": "Rust", "relation": "uses", "object": "ownership"}]
        }"#;

        let result = parse_extraction_response(json).unwrap();
        assert_eq!(result.facts.len(), 1);
        assert_eq!(result.decisions.len(), 1);
        assert!(result.predictions.is_empty());
        assert_eq!(result.relations.len(), 1);
        // Confidence clamped to DEFAULT_EXTRACTION_CONFIDENCE (0.7)
        assert!((result.facts[0].confidence - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_with_markdown_fences() {
        let json = "```json\n{\"facts\": [], \"decisions\": [], \"predictions\": [], \"relations\": []}\n```";
        let result = parse_extraction_response(json);
        assert!(result.is_some());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_parse_invalid_json() {
        let result = parse_extraction_response("not json at all");
        assert!(result.is_none());
    }

    #[test]
    fn test_empty_items_filtered() {
        let json = r#"{
            "facts": [{"content": "", "confidence": 0.5}, {"content": "valid", "confidence": 0.6}],
            "decisions": [],
            "predictions": [],
            "relations": [{"subject": "", "relation": "x", "object": "y"}]
        }"#;

        let result = parse_extraction_response(json).unwrap();
        assert_eq!(result.facts.len(), 1);
        assert_eq!(result.facts[0].content, "valid");
        assert!(result.relations.is_empty()); // Empty subject filtered
    }
}
