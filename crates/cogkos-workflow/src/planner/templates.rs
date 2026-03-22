//! Builtin workflow templates for common trigger patterns.

use regex::Regex;

use super::{PlanGenerationMethod, StepAction, WorkflowPlan, WorkflowStep, WorkflowTrigger};

pub(super) struct TemplateEntry {
    pub trigger_pattern: Regex,
    pub build_plan: fn(&WorkflowTrigger) -> WorkflowPlan,
}

pub(super) fn builtin_templates() -> Vec<TemplateEntry> {
    vec![
        TemplateEntry {
            trigger_pattern: Regex::new(r"^document_ingestion:").expect("valid regex"),
            build_plan: |_trigger| WorkflowPlan {
                name: "document_ingestion_pipeline".to_string(),
                steps: vec![
                    step("Parse Document", StepAction::ParseDocument, 30_000, 2),
                    step(
                        "Chunk Text",
                        StepAction::ChunkText {
                            max_chunk_size: 2048,
                        },
                        15_000,
                        1,
                    ),
                    step("Extract Knowledge", StepAction::ExtractKnowledge, 60_000, 2),
                    step(
                        "Generate Embeddings",
                        StepAction::GenerateEmbeddings,
                        30_000,
                        2,
                    ),
                    step("Detect Conflicts", StepAction::DetectConflicts, 30_000, 1),
                    step("Update Graph", StepAction::UpdateGraph, 15_000, 2),
                ],
                estimated_duration_ms: 180_000,
                generation_method: PlanGenerationMethod::TemplateMatch,
            },
        },
        TemplateEntry {
            trigger_pattern: Regex::new(r"^knowledge_consolidation:").expect("valid regex"),
            build_plan: |_trigger| WorkflowPlan {
                name: "knowledge_consolidation_pipeline".to_string(),
                steps: vec![
                    step(
                        "Cluster Claims",
                        StepAction::Custom {
                            description: "cluster_similar_claims".into(),
                        },
                        30_000,
                        1,
                    ),
                    step(
                        "Bayesian Aggregate",
                        StepAction::BayesianAggregate,
                        60_000,
                        2,
                    ),
                    step("Extract Insights", StepAction::ExtractKnowledge, 60_000, 2),
                    step("Update Graph", StepAction::UpdateGraph, 15_000, 2),
                    step("Cache Results", StepAction::CacheResult, 5_000, 1),
                ],
                estimated_duration_ms: 170_000,
                generation_method: PlanGenerationMethod::TemplateMatch,
            },
        },
        TemplateEntry {
            trigger_pattern: Regex::new(r"^conflict_resolution:").expect("valid regex"),
            build_plan: |_trigger| WorkflowPlan {
                name: "conflict_resolution_pipeline".to_string(),
                steps: vec![
                    step("Detect Conflicts", StepAction::DetectConflicts, 30_000, 1),
                    step("Analyze Conflicts", StepAction::ExtractKnowledge, 60_000, 2),
                    step("Bayesian Merge", StepAction::BayesianAggregate, 30_000, 2),
                    step("Update Graph", StepAction::UpdateGraph, 15_000, 2),
                ],
                estimated_duration_ms: 135_000,
                generation_method: PlanGenerationMethod::TemplateMatch,
            },
        },
    ]
}

pub(super) fn step(
    name: &str,
    action: StepAction,
    timeout_ms: u64,
    retry_count: u32,
) -> WorkflowStep {
    WorkflowStep {
        name: name.to_string(),
        action,
        timeout_ms,
        retry_count,
    }
}
