//! LLM-powered deep classification for documents
//!
//! This module provides LLM-based deep classification that extracts:
//! - Industry information
//! - Company/entity information
//! - Document type
//! - Key conclusions
//! - Predictions
//! - Data points
//! - Methodological insights

use serde::{Deserialize, Serialize};

/// LLM Client trait for deep classification
/// This is a simplified trait - actual implementation would use cogkos-llm
pub trait DeepClassificationLlmClient: Send + Sync {
    /// Generate a completion from the LLM
    fn generate(&self, prompt: &str) -> DeepClassificationResult;
}

/// Result type for LLM generation
pub struct DeepClassificationResult {
    pub content: String,
    pub confidence: f64,
}

/// Deep classification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepClassification {
    /// Industry sector (e.g., "Manufacturing", "Retail", "Finance")
    pub industry: Option<String>,
    /// Companies or entities mentioned
    pub entities: Vec<EntityMention>,
    /// Document type classification
    pub document_type: Option<String>,
    /// Key conclusions extracted
    pub conclusions: Vec<String>,
    /// Predictions mentioned in the document
    pub predictions: Vec<Prediction>,
    /// Key data points
    pub data_points: Vec<DataPoint>,
    /// Methodological insights
    pub methodologies: Vec<String>,
    /// Overall confidence in the classification
    pub confidence: f64,
}

/// Entity mention with type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityMention {
    pub name: String,
    pub entity_type: EntityType,
}

/// Type of entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EntityType {
    Company,
    Person,
    Product,
    Location,
    Organization,
    Other,
}

/// Prediction extracted from document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    pub content: String,
    pub confidence: f64,
    pub time_horizon: Option<String>,
}

/// Data point extracted from document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    pub label: String,
    pub value: String,
    pub unit: Option<String>,
    pub context: String,
}

/// Deep classifier configuration
#[derive(Debug, Clone)]
pub struct DeepClassifierConfig {
    /// Enable LLM extraction
    pub enable_llm: bool,
    /// Maximum entities to extract
    pub max_entities: usize,
    /// Minimum confidence threshold
    pub min_confidence: f64,
}

impl Default for DeepClassifierConfig {
    fn default() -> Self {
        Self {
            enable_llm: false, // Disabled by default, enable via config
            max_entities: 20,
            min_confidence: 0.5,
        }
    }
}

/// Deep classifier for LLM-based classification
pub struct DeepClassifier {
    config: DeepClassifierConfig,
}

impl DeepClassifier {
    pub fn new(config: DeepClassifierConfig) -> Self {
        Self { config }
    }

    /// Extract deep classification from document content
    ///
    /// # Arguments
    /// * `content` - Full document content
    /// * `llm_client` - Optional LLM client for extraction
    ///
    /// # Returns
    /// DeepClassification with extracted information
    pub async fn classify(
        &self,
        content: &str,
        llm_client: Option<&dyn DeepClassificationLlmClient>,
    ) -> DeepClassification {
        if self.config.enable_llm
            && let Some(client) = llm_client
        {
            return self.classify_with_llm(content, client).await;
        }

        // Fallback to rule-based extraction
        self.classify_rule_based(content)
    }

    /// LLM-based deep classification
    async fn classify_with_llm(
        &self,
        content: &str,
        client: &dyn DeepClassificationLlmClient,
    ) -> DeepClassification {
        // Build prompt for extraction
        let prompt = format!(
            r#"Extract structured information from the following document.
Respond in JSON format with these fields:
- industry: Main industry sector (or null if unclear)
- entities: Array of {{name, type}} for companies, products, persons
- document_type: Type of document (report, analysis, paper, etc.)
- conclusions: Array of key conclusions
- predictions: Array of {{content, confidence, time_horizon}} for predictions
- data_points: Array of {{label, value, unit, context}} for key metrics
- methodologies: Array of methodological approaches mentioned

Document content:
{}

Respond only with valid JSON."#,
            content.chars().take(5000).collect::<String>()
        );

        // Call LLM
        let result = client.generate(&prompt);

        // Parse JSON response
        match serde_json::from_str::<serde_json::Value>(&result.content) {
            Ok(json) => {
                let industry = json
                    .get("industry")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let document_type = json
                    .get("document_type")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                let entities: Vec<EntityMention> = json
                    .get("entities")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|e| {
                                let name = e.get("name")?.as_str()?.to_string();
                                let entity_type = match e.get("type").and_then(|v| v.as_str()) {
                                    Some("Company") => EntityType::Company,
                                    Some("Person") => EntityType::Person,
                                    Some("Product") => EntityType::Product,
                                    Some("Location") => EntityType::Location,
                                    Some("Organization") => EntityType::Organization,
                                    _ => EntityType::Other,
                                };
                                Some(EntityMention { name, entity_type })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let conclusions: Vec<String> = json
                    .get("conclusions")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();

                let predictions: Vec<Prediction> = json
                    .get("predictions")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|p| {
                                let content = p.get("content")?.as_str()?.to_string();
                                let confidence =
                                    p.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0);
                                let time_horizon = p
                                    .get("time_horizon")
                                    .and_then(|v| v.as_str())
                                    .map(String::from);
                                Some(Prediction {
                                    content,
                                    confidence,
                                    time_horizon,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let data_points: Vec<DataPoint> = json
                    .get("data_points")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|d| {
                                let label = d.get("label")?.as_str()?.to_string();
                                let value = d.get("value")?.as_str()?.to_string();
                                let unit = d.get("unit").and_then(|v| v.as_str()).map(String::from);
                                let context = d.get("context")?.as_str()?.to_string();
                                Some(DataPoint {
                                    label,
                                    value,
                                    unit,
                                    context,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let methodologies: Vec<String> = json
                    .get("methodologies")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();

                DeepClassification {
                    industry,
                    entities,
                    document_type,
                    conclusions,
                    predictions,
                    data_points,
                    methodologies,
                    confidence: result.confidence,
                }
            }
            Err(_) => {
                // Fallback to rule-based if JSON parsing fails
                self.classify_rule_based(content)
            }
        }
    }

    /// Rule-based deep classification (fallback)
    fn classify_rule_based(&self, content: &str) -> DeepClassification {
        let content_lower = content.to_lowercase();

        // Extract industry
        let industry = self.extract_industry(&content_lower);

        // Extract entities
        let entities = self.extract_entities(content);

        // Extract document type
        let document_type = self.extract_document_type(&content_lower);

        // Extract conclusions
        let conclusions = self.extract_conclusions(content);

        // Extract predictions
        let predictions = self.extract_predictions(&content_lower);

        // Extract data points
        let data_points = self.extract_data_points(content);

        // Extract methodologies
        let methodologies = self.extract_methodologies(&content_lower);

        DeepClassification {
            industry,
            entities,
            document_type,
            conclusions,
            predictions,
            data_points,
            methodologies,
            confidence: 0.6, // Rule-based has lower confidence
        }
    }

    fn extract_industry(&self, content: &str) -> Option<String> {
        // Chinese keyword matching (priority)
        let chinese_industries = [
            ("制造业", "制造业"),
            ("生产制造", "制造业"),
            ("零售", "零售业"),
            ("金融", "金融业"),
            ("银行", "金融业"),
            ("医疗", "医疗健康"),
            ("健康", "医疗健康"),
            ("科技", "科技业"),
            ("软件", "软件业"),
            ("能源", "能源业"),
            ("汽车制造", "汽车制造业"),
        ];

        for (keyword, industry) in chinese_industries {
            if content.contains(keyword) {
                return Some(industry.to_string());
            }
        }

        // English keyword matching
        let industries = [
            ("manufacturing", "制造业"),
            ("manufacture", "制造业"),
            ("retail", "零售业"),
            ("finance", "金融业"),
            ("banking", "金融业"),
            ("healthcare", "医疗健康"),
            ("technology", "科技业"),
            ("software", "软件业"),
            ("energy", "能源业"),
            ("automotive", "汽车制造业"),
        ];

        for (keyword, industry) in industries {
            if content.contains(keyword) {
                return Some(industry.to_string());
            }
        }
        None
    }

    fn extract_entities(&self, content: &str) -> Vec<EntityMention> {
        let mut entities = Vec::new();

        // Simple entity extraction patterns
        // Company patterns
        let company_patterns = ["公司", "集团", "企业", "Inc", "Corp", "Ltd"];
        for pattern in company_patterns {
            if content.contains(pattern) {
                // Simplified extraction
                entities.push(EntityMention {
                    name: pattern.to_string(),
                    entity_type: EntityType::Company,
                });
            }
        }

        entities.truncate(self.config.max_entities);
        entities
    }

    fn extract_document_type(&self, content: &str) -> Option<String> {
        if content.contains("年报") || content.contains("annual report") {
            Some("年度报告".to_string())
        } else if content.contains("季报") || content.contains("quarterly") {
            Some("季度报告".to_string())
        } else if content.contains("分析") || content.contains("analysis") {
            Some("分析报告".to_string())
        } else if content.contains("研究") || content.contains("research") {
            Some("研究报告".to_string())
        } else {
            Some("文档".to_string())
        }
    }

    fn extract_conclusions(&self, content: &str) -> Vec<String> {
        let mut conclusions = Vec::new();

        // Look for conclusion markers
        let markers = ["结论", "结论如下", "总结", "综上", "因此", "总之"];

        for marker in markers {
            if content.contains(marker) {
                // Simplified extraction
                conclusions.push(marker.to_string());
            }
        }

        conclusions
    }

    fn extract_predictions(&self, content: &str) -> Vec<Prediction> {
        let mut predictions = Vec::new();

        // Look for prediction markers
        let markers = ["预计", "预测", "预期", "将", "will", "expect", "forecast"];

        for marker in markers {
            if content.contains(marker) {
                predictions.push(Prediction {
                    content: marker.to_string(),
                    confidence: 0.5,
                    time_horizon: None,
                });
            }
        }

        predictions
    }

    fn extract_data_points(&self, content: &str) -> Vec<DataPoint> {
        // Look for numeric patterns that might be data points
        let mut data_points = Vec::new();

        // Simple percentage extraction
        let percent_matches: Vec<_> = content.match_indices("%").collect();
        for (pos, _) in percent_matches.iter().take(10) {
            let start = pos.saturating_sub(20);
            let end = (pos + 1).min(content.len());
            let context = &content[start..end];

            data_points.push(DataPoint {
                label: "percentage".to_string(),
                value: context.to_string(),
                unit: Some("%".to_string()),
                context: context.to_string(),
            });
        }

        data_points
    }

    fn extract_methodologies(&self, content: &str) -> Vec<String> {
        let mut methodologies = Vec::new();

        let method_markers = [
            "方法论",
            "methodology",
            "研究方法",
            "分析框架",
            "模型",
            "approach",
            "framework",
        ];

        for marker in method_markers {
            if content.contains(marker) {
                methodologies.push(marker.to_string());
            }
        }

        methodologies
    }
}

impl Default for DeepClassifier {
    fn default() -> Self {
        Self::new(DeepClassifierConfig::default())
    }
}

/// Convert deep classification to graph relations
impl DeepClassification {
    /// Generate graph relation suggestions based on classification
    pub fn to_graph_relations(&self, file_claim_id: &str) -> Vec<GraphRelationSuggestion> {
        let mut relations = Vec::new();

        // File -> Industry
        if let Some(ref industry) = self.industry {
            relations.push(GraphRelationSuggestion {
                from_id: file_claim_id.to_string(),
                to_label: industry.clone(),
                relation_type: "INDUSTRY".to_string(),
            });
        }

        // File -> Entity
        for entity in &self.entities {
            relations.push(GraphRelationSuggestion {
                from_id: file_claim_id.to_string(),
                to_label: entity.name.clone(),
                relation_type: "ABOUT".to_string(),
            });
        }

        // File -> Conclusions
        for conclusion in &self.conclusions {
            relations.push(GraphRelationSuggestion {
                from_id: file_claim_id.to_string(),
                to_label: conclusion.clone(),
                relation_type: "CONTAINS".to_string(),
            });
        }

        // File -> Predictions
        for prediction in &self.predictions {
            relations.push(GraphRelationSuggestion {
                from_id: file_claim_id.to_string(),
                to_label: prediction.content.clone(),
                relation_type: "CONTAINS_PREDICTION".to_string(),
            });
        }

        relations
    }
}

/// Graph relation suggestion
#[derive(Debug, Clone)]
pub struct GraphRelationSuggestion {
    pub from_id: String,
    pub to_label: String,
    pub relation_type: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_based_classification() {
        let classifier = DeepClassifier::default();
        let content = "这是某公司2025年年度报告。公司属于制造业，预计2026年营收增长10%。";

        let result = classifier.classify_rule_based(content);

        assert!(result.industry.is_some());
        assert!(!result.entities.is_empty());
        assert!(!result.predictions.is_empty());
    }

    #[test]
    fn test_graph_relations() {
        let classification = DeepClassification {
            industry: Some("制造业".to_string()),
            entities: vec![EntityMention {
                name: "某公司".to_string(),
                entity_type: EntityType::Company,
            }],
            document_type: Some("年度报告".to_string()),
            conclusions: vec!["结论1".to_string()],
            predictions: vec![Prediction {
                content: "增长10%".to_string(),
                confidence: 0.8,
                time_horizon: Some("2026".to_string()),
            }],
            data_points: vec![],
            methodologies: vec![],
            confidence: 0.7,
        };

        let relations = classification.to_graph_relations("file-123");
        assert!(!relations.is_empty());
    }
}
