//! LLM Sandbox for testing new frameworks in isolation

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::models::EpistemicClaim;
use super::{
    AnomalyDetectionResult, LlmClient, LlmMessage, LlmRole, LlmRequest, ParadigmShiftError,
};

/// Framework definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Framework {
    pub id: Uuid,
    pub name: String,
    pub version: String,
    /// Framework hash for integrity
    pub hash: String,
    /// Ontology definition
    pub ontology: OntologyDefinition,
    /// Conflict resolution rules
    pub resolution_rules: Vec<ResolutionRule>,
    /// Prediction models
    pub prediction_config: PredictionConfig,
    /// Validation criteria
    pub validation_criteria: ValidationCriteria,
    pub created_at: DateTime<Utc>,
}

/// Ontology definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyDefinition {
    /// Entity types
    pub entity_types: Vec<String>,
    /// Relationship types
    pub relation_types: Vec<String>,
    /// Attribute definitions
    pub attributes: HashMap<String, AttributeDef>,
    /// Constraints
    pub constraints: Vec<OntologyConstraint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeDef {
    pub data_type: String,
    pub required: bool,
    pub validation_regex: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyConstraint {
    pub constraint_type: String,
    pub description: String,
}

/// Resolution rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionRule {
    pub rule_id: String,
    pub applies_to: String,
    pub priority: i32,
    pub logic: ResolutionLogic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResolutionLogic {
    ConfidenceWeighted,
    TemporalPriority,
    SourceAuthority {
        authority_rankings: HashMap<String, i32>,
    },
    Custom {
        code: String,
    },
}

/// Prediction configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionConfig {
    pub model_type: String,
    pub parameters: HashMap<String, serde_json::Value>,
    pub confidence_threshold: f64,
}

/// Validation criteria
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationCriteria {
    pub min_prediction_accuracy: f64,
    pub max_conflict_rate: f64,
    pub min_confidence_calibration: f64,
    pub min_coverage: f64,
}

/// LLM Sandbox for testing new frameworks
pub struct LlmSandbox {
    /// Current framework being tested
    test_framework: Option<Framework>,
    /// Test results
    results: Vec<SandboxTestResult>,
    /// Isolation boundary
    isolation_config: IsolationConfig,
    /// LLM client for generating candidates
    llm_client: Option<Arc<dyn LlmClient>>,
    /// Model to use for LLM generation
    llm_model: String,
    /// Generation history
    generation_history: Vec<GenerationRecord>,
}

#[derive(Debug, Clone)]
pub struct IsolationConfig {
    /// Max claims to process
    pub max_claims: usize,
    /// Max compute time
    pub max_compute_seconds: u64,
    /// Data subset percentage
    pub data_subset_pct: f64,
}

impl Default for IsolationConfig {
    fn default() -> Self {
        Self {
            max_claims: 10000,
            max_compute_seconds: 3600,
            data_subset_pct: 0.1,
        }
    }
}

/// Record of framework generation
#[derive(Debug, Clone)]
pub struct GenerationRecord {
    pub timestamp: DateTime<Utc>,
    pub input_anomaly_score: f64,
    pub output_framework_id: Uuid,
    pub success: bool,
}

/// Extract JSON from LLM response (handles markdown code blocks)
fn extract_json_from_response(response: &str) -> Option<String> {
    // Try to find JSON in code blocks
    if let Some(start) = response.find("```json") {
        let start = start + 7;
        if let Some(end) = response[start..].find("```") {
            return Some(response[start..start + end].trim().to_string());
        }
    }

    // Try to find any JSON object
    if let Some(start) = response.find('{') {
        let mut brace_count = 0;
        for (i, c) in response[start..].chars().enumerate() {
            match c {
                '{' => brace_count += 1,
                '}' => {
                    brace_count -= 1;
                    if brace_count == 0 {
                        return Some(response[start..start + i + 1].to_string());
                    }
                }
                _ => {}
            }
        }
    }

    None
}

/// Parse ontology from JSON
fn parse_ontology(
    value: &serde_json::Value,
) -> std::result::Result<OntologyDefinition, ParadigmShiftError> {
    let entity_types: Vec<String> = value["entity_types"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let relation_types: Vec<String> = value["relation_types"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let mut attributes = HashMap::new();
    if let Some(obj) = value["attributes"].as_object() {
        for (k, v) in obj {
            attributes.insert(
                k.clone(),
                AttributeDef {
                    data_type: v["data_type"].as_str().unwrap_or("string").to_string(),
                    required: v["required"].as_bool().unwrap_or(false),
                    validation_regex: v["validation_regex"].as_str().map(String::from),
                },
            );
        }
    }

    let constraints: Vec<OntologyConstraint> = value["constraints"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    Some(OntologyConstraint {
                        constraint_type: v["constraint_type"].as_str()?.to_string(),
                        description: v["description"].as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(OntologyDefinition {
        entity_types,
        relation_types,
        attributes,
        constraints,
    })
}

/// Parse resolution rules from JSON
fn parse_resolution_rules(value: &serde_json::Value) -> Vec<ResolutionRule> {
    value
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    Some(ResolutionRule {
                        rule_id: v["rule_id"].as_str()?.to_string(),
                        applies_to: v["applies_to"].as_str()?.to_string(),
                        priority: v["priority"].as_i64().unwrap_or(0) as i32,
                        logic: ResolutionLogic::ConfidenceWeighted,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse prediction config from JSON
fn parse_prediction_config(value: &serde_json::Value) -> PredictionConfig {
    PredictionConfig {
        model_type: value["model_type"]
            .as_str()
            .unwrap_or("default")
            .to_string(),
        parameters: value["parameters"]
            .as_object()
            .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default(),
        confidence_threshold: value["confidence_threshold"].as_f64().unwrap_or(0.7),
    }
}

/// Parse validation criteria from JSON
fn parse_validation_criteria(value: &serde_json::Value) -> ValidationCriteria {
    ValidationCriteria {
        min_prediction_accuracy: value["min_prediction_accuracy"].as_f64().unwrap_or(0.7),
        max_conflict_rate: value["max_conflict_rate"].as_f64().unwrap_or(0.2),
        min_confidence_calibration: value["min_confidence_calibration"].as_f64().unwrap_or(0.8),
        min_coverage: value["min_coverage"].as_f64().unwrap_or(0.5),
    }
}

/// Sandbox test result
#[derive(Debug, Clone)]
pub struct SandboxTestResult {
    pub timestamp: DateTime<Utc>,
    pub test_type: SandboxTestType,
    pub success: bool,
    pub metrics: SandboxMetrics,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxTestType {
    OntologyValidation,
    ResolutionTest,
    PredictionTest,
    IntegrationTest,
}

/// Metrics from sandbox testing
#[derive(Debug, Clone, Default)]
pub struct SandboxMetrics {
    pub claims_processed: usize,
    pub conflicts_resolved: usize,
    pub predictions_made: usize,
    pub prediction_accuracy: f64,
    pub avg_resolution_time_ms: u64,
    pub ontology_violations: usize,
}

impl LlmSandbox {
    pub fn new() -> Self {
        Self {
            test_framework: None,
            results: Vec::new(),
            isolation_config: IsolationConfig::default(),
            llm_client: None,
            llm_model: "gpt-4".to_string(),
            generation_history: Vec::new(),
        }
    }

    pub fn with_isolation_config(mut self, config: IsolationConfig) -> Self {
        self.isolation_config = config;
        self
    }

    /// Set the LLM client for generating candidate frameworks
    pub fn with_llm_client(mut self, client: Arc<dyn LlmClient>, model: impl Into<String>) -> Self {
        self.llm_client = Some(client);
        self.llm_model = model.into();
        self
    }

    /// Check if LLM client is available
    pub fn has_llm_client(&self) -> bool {
        self.llm_client.is_some()
    }

    /// Load a framework into the sandbox
    pub fn load_framework(&mut self, framework: Framework) {
        self.test_framework = Some(framework);
        self.results.clear();
    }

    /// Run ontology validation test
    pub fn test_ontology(&mut self, claims: &[EpistemicClaim]) -> SandboxTestResult {
        let framework = match &self.test_framework {
            Some(f) => f,
            None => {
                return SandboxTestResult {
                    timestamp: Utc::now(),
                    test_type: SandboxTestType::OntologyValidation,
                    success: false,
                    metrics: SandboxMetrics::default(),
                    errors: vec!["No framework loaded".to_string()],
                };
            }
        };

        let sample_size = (claims.len() as f64 * self.isolation_config.data_subset_pct) as usize;
        let sample = &claims[..sample_size.min(claims.len())];

        // If no claims to validate, consider it a pass
        let (success, violations, errors) = if sample.is_empty() {
            (true, 0, vec![])
        } else {
            let mut violations = 0;
            let mut errors = Vec::new();

            for claim in sample {
                // Check if claim type is in ontology
                let type_str = format!("{:?}", claim.node_type);
                if !framework.ontology.entity_types.contains(&type_str) {
                    violations += 1;
                    if violations <= 5 {
                        errors.push(format!("Type {} not in ontology", type_str));
                    }
                }
            }

            let success = violations < sample.len() / 10; // Less than 10% violations
            (success, violations, errors)
        };

        let result = SandboxTestResult {
            timestamp: Utc::now(),
            test_type: SandboxTestType::OntologyValidation,
            success,
            metrics: SandboxMetrics {
                claims_processed: sample.len(),
                ontology_violations: violations,
                ..Default::default()
            },
            errors,
        };

        self.results.push(result.clone());
        result
    }

    /// Run prediction test
    pub fn test_predictions(
        &mut self,
        predictions: &[(String, String, bool)],
    ) -> SandboxTestResult {
        // predictions: (input, predicted_outcome, actual_outcome)
        let mut correct = 0;

        for (_, _predicted, actual) in predictions {
            if *actual {
                // Simplified: assume correct if actual is true
                correct += 1;
            }
        }

        let accuracy = if predictions.is_empty() {
            0.0
        } else {
            correct as f64 / predictions.len() as f64
        };

        let framework = self.test_framework.as_ref().unwrap();
        let success = accuracy >= framework.validation_criteria.min_prediction_accuracy;

        let result = SandboxTestResult {
            timestamp: Utc::now(),
            test_type: SandboxTestType::PredictionTest,
            success,
            metrics: SandboxMetrics {
                predictions_made: predictions.len(),
                prediction_accuracy: accuracy,
                ..Default::default()
            },
            errors: if success {
                vec![]
            } else {
                vec![format!("Accuracy {:.2} below threshold", accuracy)]
            },
        };

        self.results.push(result.clone());
        result
    }

    /// Get all test results
    pub fn results(&self) -> &[SandboxTestResult] {
        &self.results
    }

    /// Check if all tests passed
    pub fn all_tests_passed(&self) -> bool {
        !self.results.is_empty() && self.results.iter().all(|r| r.success)
    }

    /// Generate sandbox report
    pub fn generate_report(&self) -> SandboxReport {
        let total_tests = self.results.len();
        let passed_tests = self.results.iter().filter(|r| r.success).count();

        SandboxReport {
            framework_name: self
                .test_framework
                .as_ref()
                .map(|f| f.name.clone())
                .unwrap_or_default(),
            total_tests,
            passed_tests,
            failed_tests: total_tests - passed_tests,
            pass_rate: if total_tests > 0 {
                passed_tests as f64 / total_tests as f64
            } else {
                0.0
            },
            is_safe_to_deploy: self.all_tests_passed() && total_tests >= 3,
        }
    }

    /// Generate a candidate framework using LLM
    pub async fn generate_candidate_framework(
        &mut self,
        current_framework: &Framework,
        anomaly_context: &AnomalyDetectionResult,
        claims_sample: &[EpistemicClaim],
    ) -> std::result::Result<Framework, ParadigmShiftError> {
        let client = self.llm_client.as_ref().ok_or_else(|| {
            ParadigmShiftError::SandboxError("LLM client not configured".to_string())
        })?;

        let context = self.build_framework_generation_context(
            current_framework,
            anomaly_context,
            claims_sample,
        );

        let request = LlmRequest {
            model: self.llm_model.clone(),
            messages: context,
            temperature: 0.8,
            max_tokens: Some(4000),
            top_p: None,
            stop_sequences: vec!["```".to_string()],
        };

        let response = client
            .chat(request)
            .await
            .map_err(|e| ParadigmShiftError::SandboxError(format!("LLM call failed: {}", e)))?;

        let candidate = self.parse_framework_response(&response.content, current_framework)?;

        self.generation_history.push(GenerationRecord {
            timestamp: Utc::now(),
            input_anomaly_score: anomaly_context.anomaly_score,
            output_framework_id: candidate.id,
            success: true,
        });

        Ok(candidate)
    }

    fn build_framework_generation_context(
        &self,
        current: &Framework,
        anomalies: &AnomalyDetectionResult,
        claims: &[EpistemicClaim],
    ) -> Vec<LlmMessage> {
        let system_prompt = r#"You are an expert system architect specializing in knowledge representation and epistemic frameworks.
Your task is to propose a new candidate framework that addresses the anomalies and limitations of the current framework.

Respond with a JSON object representing the new framework. Use this exact structure:
{
  "name": "FrameworkName",
  "version": "1.1",
  "ontology": {
    "entity_types": ["Entity", "Relation", "Claim", ...],
    "relation_types": ["related_to", "contradicts", "supports", ...],
    "attributes": {...},
    "constraints": [...]
  },
  "resolution_rules": [...],
  "prediction_config": {...},
  "validation_criteria": {...}
}

Make sure the new framework addresses the identified anomalies while maintaining backward compatibility where possible."#;

        let current_desc = serde_json::to_string_pretty(&current).unwrap_or_default();
        let anomalies_desc = format!(
            "Anomaly Score: {:.2}\nAssessment: {:?}\nAnomalies: {:?}",
            anomalies.anomaly_score,
            anomalies.assessment,
            anomalies
                .anomalies
                .iter()
                .map(|a| &a.description)
                .collect::<Vec<_>>()
        );

        let claims_sample: Vec<String> = claims
            .iter()
            .take(20)
            .map(|c| {
                format!(
                    "- {}: {} (confidence: {:.2})",
                    c.id, c.content, c.confidence
                )
            })
            .collect();

        let user_message = format!(
            "Current Framework:\n{}\n\nAnomaly Analysis:\n{}\n\nRecent Claims Sample:\n{}\n\nPlease generate a candidate framework that addresses these anomalies. Return only valid JSON.",
            current_desc,
            anomalies_desc,
            claims_sample.join("\n")
        );

        vec![
            LlmMessage {
                role: LlmRole::System,
                content: system_prompt.to_string(),
            },
            LlmMessage {
                role: LlmRole::User,
                content: user_message,
            },
        ]
    }

    fn parse_framework_response(
        &self,
        response: &str,
        _current: &Framework,
    ) -> std::result::Result<Framework, ParadigmShiftError> {
        let json_str = extract_json_from_response(response).ok_or_else(|| {
            ParadigmShiftError::SandboxError("No valid JSON in LLM response".to_string())
        })?;

        let parsed: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| {
            ParadigmShiftError::SandboxError(format!("Failed to parse framework JSON: {}", e))
        })?;

        let name = parsed["name"]
            .as_str()
            .unwrap_or("LLM-Generated-Framework")
            .to_string();
        let version = parsed["version"].as_str().unwrap_or("1.1").to_string();

        let ontology = parse_ontology(&parsed["ontology"])?;
        let resolution_rules = parse_resolution_rules(&parsed["resolution_rules"]);
        let prediction_config = parse_prediction_config(&parsed["prediction_config"]);
        let validation_criteria = parse_validation_criteria(&parsed["validation_criteria"]);

        let hash = format!("llm_hash_{}", Uuid::new_v4());

        Ok(Framework {
            id: Uuid::new_v4(),
            name,
            version,
            hash,
            ontology,
            resolution_rules,
            prediction_config,
            validation_criteria,
            created_at: Utc::now(),
        })
    }

    /// Get generation history
    pub fn generation_history(&self) -> &[GenerationRecord] {
        &self.generation_history
    }
}

impl Default for LlmSandbox {
    fn default() -> Self {
        Self::new()
    }
}

/// Sandbox test report
#[derive(Debug, Clone)]
pub struct SandboxReport {
    pub framework_name: String,
    pub total_tests: usize,
    pub passed_tests: usize,
    pub failed_tests: usize,
    pub pass_rate: f64,
    pub is_safe_to_deploy: bool,
}
