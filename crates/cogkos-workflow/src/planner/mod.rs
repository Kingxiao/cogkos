//! LLM-powered workflow planner — auto-generates workflow plans from triggers.
//!
//! Strategy: template matching first (fast, no LLM), LLM generation fallback.

mod templates;

use crate::dsl::{
    EdgeDefinition, EdgeType, NodeDefinition, NodeType, RetryPolicy, WorkflowDefinition,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use templates::step;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Task types that trigger automatic workflow generation
#[derive(Debug, Clone)]
pub enum WorkflowTrigger {
    /// Document uploaded — needs parse -> chunk -> embed -> conflict detect
    DocumentIngestion {
        filename: String,
        content_type: String,
    },
    /// Multiple claims need consolidation
    KnowledgeConsolidation {
        tenant_id: String,
        claim_count: usize,
    },
    /// Complex query requiring multi-step processing
    ComplexQuery { query: String, domains: Vec<String> },
    /// Batch of conflicts need resolution
    ConflictResolution {
        tenant_id: String,
        conflict_count: usize,
    },
    /// Custom task described in natural language
    Custom { description: String },
}

/// Generated workflow plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPlan {
    pub name: String,
    pub steps: Vec<WorkflowStep>,
    pub estimated_duration_ms: u64,
    pub generation_method: PlanGenerationMethod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanGenerationMethod {
    TemplateMatch,
    LlmGenerated,
    DefaultFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub name: String,
    pub action: StepAction,
    pub timeout_ms: u64,
    pub retry_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StepAction {
    ParseDocument,
    ChunkText { max_chunk_size: usize },
    GenerateEmbeddings,
    DetectConflicts,
    ExtractKnowledge,
    BayesianAggregate,
    UpdateGraph,
    CacheResult,
    Custom { description: String },
}

/// Minimal LLM trait — avoids coupling to cogkos-llm directly.
#[async_trait::async_trait]
pub trait PlannerLlmClient: Send + Sync {
    async fn generate(&self, prompt: &str) -> Result<String, String>;
}

pub struct WorkflowPlanner {
    llm_client: Option<Arc<dyn PlannerLlmClient>>,
    templates: Vec<templates::TemplateEntry>,
}

impl WorkflowPlanner {
    pub fn new(llm_client: Option<Arc<dyn PlannerLlmClient>>) -> Self {
        let tpls = templates::builtin_templates();
        Self {
            llm_client,
            templates: tpls,
        }
    }

    /// Auto-generate workflow plan for a given trigger
    pub async fn plan(&self, trigger: &WorkflowTrigger) -> WorkflowPlan {
        if let Some(plan) = self.match_template(trigger) {
            info!(plan = %plan.name, "Matched builtin template");
            return plan;
        }
        if let Some(ref client) = self.llm_client {
            match self.llm_generate(client.as_ref(), trigger).await {
                Some(plan) => {
                    info!(plan = %plan.name, "LLM generated workflow plan");
                    return plan;
                }
                None => warn!("LLM plan generation failed, using default fallback"),
            }
        }
        self.default_plan(trigger)
    }

    /// Convert plan into executable WorkflowDefinition (for the engine)
    pub fn to_definition(&self, plan: &WorkflowPlan) -> WorkflowDefinition {
        let nodes: Vec<NodeDefinition> = plan
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| NodeDefinition {
                id: format!("step_{}", i),
                node_type: NodeType::Task,
                name: Some(s.name.clone()),
                description: Some(format!("{:?}", s.action)),
                config: serde_json::to_value(&s.action).unwrap_or_default(),
                retry_policy: if s.retry_count > 0 {
                    Some(RetryPolicy {
                        max_attempts: s.retry_count,
                        ..RetryPolicy::default()
                    })
                } else {
                    None
                },
                timeout_seconds: Some(s.timeout_ms / 1000),
                condition: None,
            })
            .collect();

        let edges: Vec<EdgeDefinition> = (0..nodes.len().saturating_sub(1))
            .map(|i| EdgeDefinition {
                from: format!("step_{}", i),
                to: format!("step_{}", i + 1),
                edge_type: EdgeType::Sequential,
                condition: None,
            })
            .collect();

        WorkflowDefinition {
            id: Uuid::new_v4().to_string(),
            name: plan.name.clone(),
            version: "1.0".to_string(),
            description: Some(format!("Auto-generated ({:?})", plan.generation_method)),
            nodes,
            edges,
            variables: HashMap::new(),
            timeout_seconds: Some(plan.estimated_duration_ms / 1000),
            retry_policy: Some(RetryPolicy::default()),
        }
    }

    fn match_template(&self, trigger: &WorkflowTrigger) -> Option<WorkflowPlan> {
        let key = Self::trigger_key(trigger);
        self.templates
            .iter()
            .find(|e| e.trigger_pattern.is_match(&key))
            .map(|e| (e.build_plan)(trigger))
    }

    fn trigger_key(trigger: &WorkflowTrigger) -> String {
        match trigger {
            WorkflowTrigger::DocumentIngestion { content_type, .. } => {
                format!("document_ingestion:{}", content_type)
            }
            WorkflowTrigger::KnowledgeConsolidation { claim_count, .. } => {
                format!("knowledge_consolidation:{}", claim_count)
            }
            WorkflowTrigger::ComplexQuery { .. } => "complex_query".to_string(),
            WorkflowTrigger::ConflictResolution { conflict_count, .. } => {
                format!("conflict_resolution:{}", conflict_count)
            }
            WorkflowTrigger::Custom { description } => format!("custom:{}", description),
        }
    }

    async fn llm_generate(
        &self,
        client: &dyn PlannerLlmClient,
        trigger: &WorkflowTrigger,
    ) -> Option<WorkflowPlan> {
        let prompt = format!(
            "Generate a workflow plan as JSON for this task:\n{}\n\n\
             Available actions: ParseDocument, ChunkText, GenerateEmbeddings, \
             DetectConflicts, ExtractKnowledge, BayesianAggregate, UpdateGraph, CacheResult.\n\n\
             Respond with JSON only:\n\
             {{\"name\": \"...\", \"steps\": [{{\"name\": \"...\", \"action\": \"...\", \
             \"timeout_ms\": 30000, \"retry_count\": 1}}], \"estimated_duration_ms\": 60000}}",
            Self::trigger_description(trigger)
        );
        let response = client.generate(&prompt).await.ok()?;
        debug!(response_len = response.len(), "LLM plan response received");
        Self::parse_llm_response(&response)
    }

    fn trigger_description(trigger: &WorkflowTrigger) -> String {
        match trigger {
            WorkflowTrigger::DocumentIngestion {
                filename,
                content_type,
            } => format!(
                "Ingest document '{}' (type: {}). Parse, chunk, embed, detect conflicts, store.",
                filename, content_type
            ),
            WorkflowTrigger::KnowledgeConsolidation { claim_count, .. } => format!(
                "Consolidate {} claims. Cluster, aggregate via Bayesian inference, extract insights.",
                claim_count
            ),
            WorkflowTrigger::ComplexQuery { query, domains } => format!(
                "Process complex query '{}' across domains: {:?}.",
                query, domains
            ),
            WorkflowTrigger::ConflictResolution { conflict_count, .. } => format!(
                "Resolve {} conflicts. Analyze, suggest resolutions, merge.",
                conflict_count
            ),
            WorkflowTrigger::Custom { description } => description.clone(),
        }
    }

    fn parse_llm_response(response: &str) -> Option<WorkflowPlan> {
        let json_str = if let Some(start) = response.find('{') {
            let end = response.rfind('}')?;
            &response[start..=end]
        } else {
            return None;
        };

        #[derive(Deserialize)]
        struct RawPlan {
            name: String,
            steps: Vec<RawStep>,
            estimated_duration_ms: Option<u64>,
        }
        #[derive(Deserialize)]
        struct RawStep {
            name: String,
            action: String,
            timeout_ms: Option<u64>,
            retry_count: Option<u32>,
        }

        let raw: RawPlan = serde_json::from_str(json_str).ok()?;
        let steps = raw
            .steps
            .into_iter()
            .map(|s| {
                let action = match s.action.to_lowercase().as_str() {
                    "parsedocument" | "parse_document" => StepAction::ParseDocument,
                    "chunktext" | "chunk_text" => StepAction::ChunkText {
                        max_chunk_size: 2048,
                    },
                    "generateembeddings" | "generate_embeddings" => StepAction::GenerateEmbeddings,
                    "detectconflicts" | "detect_conflicts" => StepAction::DetectConflicts,
                    "extractknowledge" | "extract_knowledge" => StepAction::ExtractKnowledge,
                    "bayesianaggregate" | "bayesian_aggregate" => StepAction::BayesianAggregate,
                    "updategraph" | "update_graph" => StepAction::UpdateGraph,
                    "cacheresult" | "cache_result" => StepAction::CacheResult,
                    _ => StepAction::Custom {
                        description: s.action,
                    },
                };
                WorkflowStep {
                    name: s.name,
                    action,
                    timeout_ms: s.timeout_ms.unwrap_or(30_000),
                    retry_count: s.retry_count.unwrap_or(1),
                }
            })
            .collect();

        Some(WorkflowPlan {
            name: raw.name,
            steps,
            estimated_duration_ms: raw.estimated_duration_ms.unwrap_or(60_000),
            generation_method: PlanGenerationMethod::LlmGenerated,
        })
    }

    fn default_plan(&self, trigger: &WorkflowTrigger) -> WorkflowPlan {
        let (name, steps) = match trigger {
            WorkflowTrigger::DocumentIngestion { .. } => (
                "default_document_ingestion",
                vec![step("Parse", StepAction::ParseDocument, 30_000, 1)],
            ),
            _ => (
                "default_generic",
                vec![step(
                    "Process",
                    StepAction::Custom {
                        description: Self::trigger_description(trigger),
                    },
                    60_000,
                    1,
                )],
            ),
        };
        WorkflowPlan {
            name: name.to_string(),
            steps,
            estimated_duration_ms: 30_000,
            generation_method: PlanGenerationMethod::DefaultFallback,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_template_match_document_ingestion() {
        let planner = WorkflowPlanner::new(None);
        let trigger = WorkflowTrigger::DocumentIngestion {
            filename: "report.pdf".into(),
            content_type: "application/pdf".into(),
        };
        let plan = planner.plan(&trigger).await;
        assert_eq!(plan.name, "document_ingestion_pipeline");
        assert_eq!(plan.generation_method, PlanGenerationMethod::TemplateMatch);
        assert_eq!(plan.steps.len(), 6);
        assert!(matches!(plan.steps[0].action, StepAction::ParseDocument));
        assert!(matches!(plan.steps[4].action, StepAction::DetectConflicts));
    }

    #[tokio::test]
    async fn test_template_match_consolidation() {
        let planner = WorkflowPlanner::new(None);
        let trigger = WorkflowTrigger::KnowledgeConsolidation {
            tenant_id: "test".into(),
            claim_count: 50,
        };
        let plan = planner.plan(&trigger).await;
        assert_eq!(plan.name, "knowledge_consolidation_pipeline");
        assert_eq!(plan.generation_method, PlanGenerationMethod::TemplateMatch);
        assert!(plan.steps.len() >= 4);
    }

    #[tokio::test]
    async fn test_default_fallback_for_custom_trigger() {
        let planner = WorkflowPlanner::new(None);
        let trigger = WorkflowTrigger::Custom {
            description: "do something unusual".into(),
        };
        let plan = planner.plan(&trigger).await;
        assert_eq!(
            plan.generation_method,
            PlanGenerationMethod::DefaultFallback
        );
        assert!(!plan.steps.is_empty());
    }

    #[test]
    fn test_builtin_templates_nonempty() {
        let templates = templates::builtin_templates();
        assert!(templates.len() >= 3);
    }

    #[test]
    fn test_plan_to_definition() {
        let planner = WorkflowPlanner::new(None);
        let plan = WorkflowPlan {
            name: "test_plan".into(),
            steps: vec![
                step("A", StepAction::ParseDocument, 10_000, 1),
                step("B", StepAction::GenerateEmbeddings, 20_000, 2),
            ],
            estimated_duration_ms: 30_000,
            generation_method: PlanGenerationMethod::TemplateMatch,
        };
        let def = planner.to_definition(&plan);
        assert_eq!(def.nodes.len(), 2);
        assert_eq!(def.edges.len(), 1);
        assert_eq!(def.edges[0].from, "step_0");
        assert_eq!(def.edges[0].to, "step_1");
    }

    #[test]
    fn test_parse_llm_response_valid() {
        let json = r#"{"name": "test", "steps": [{"name": "step1", "action": "ParseDocument", "timeout_ms": 5000, "retry_count": 1}], "estimated_duration_ms": 5000}"#;
        let plan = WorkflowPlanner::parse_llm_response(json).unwrap();
        assert_eq!(plan.name, "test");
        assert!(matches!(plan.steps[0].action, StepAction::ParseDocument));
        assert_eq!(plan.generation_method, PlanGenerationMethod::LlmGenerated);
    }

    #[test]
    fn test_parse_llm_response_with_fences() {
        let response = "```json\n{\"name\": \"fenced\", \"steps\": [{\"name\": \"s\", \"action\": \"cache_result\"}]}\n```";
        let plan = WorkflowPlanner::parse_llm_response(response).unwrap();
        assert_eq!(plan.name, "fenced");
        assert!(matches!(plan.steps[0].action, StepAction::CacheResult));
    }

    #[test]
    fn test_parse_llm_response_invalid() {
        assert!(WorkflowPlanner::parse_llm_response("not json").is_none());
        assert!(WorkflowPlanner::parse_llm_response("").is_none());
    }
}
