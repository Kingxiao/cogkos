use super::{
    EdgeDefinition, EdgeType, NodeDefinition, NodeType, RetryPolicy, WorkflowDefinition,
};
use std::collections::HashMap;
use uuid::Uuid;

/// Predefined workflow templates
pub struct WorkflowTemplates;

impl WorkflowTemplates {
    /// Create a conflict detection workflow
    pub fn conflict_detection() -> WorkflowDefinition {
        WorkflowDefinition {
            id: Uuid::new_v4().to_string(),
            name: "conflict_detection".to_string(),
            version: "1.0".to_string(),
            description: Some("Detect and analyze conflicts between claims".to_string()),
            nodes: vec![
                NodeDefinition {
                    id: "fetch_claims".to_string(),
                    node_type: NodeType::Task,
                    name: Some("Fetch Claims".to_string()),
                    description: Some(
                        "Retrieve candidate claims for conflict detection".to_string(),
                    ),
                    config: serde_json::json!({
                        "query": "SELECT * FROM epistemic_claims WHERE status = 'active'"
                    }),
                    retry_policy: None,
                    timeout_seconds: Some(30),
                    condition: None,
                },
                NodeDefinition {
                    id: "detect_conflicts".to_string(),
                    node_type: NodeType::ConflictDetect,
                    name: Some("Detect Conflicts".to_string()),
                    description: Some("Find contradictory claims".to_string()),
                    config: serde_json::json!({}),
                    retry_policy: None,
                    timeout_seconds: Some(60),
                    condition: None,
                },
                NodeDefinition {
                    id: "analyze_density".to_string(),
                    node_type: NodeType::Task,
                    name: Some("Analyze Conflict Density".to_string()),
                    description: Some("Check if conflict density exceeds threshold".to_string()),
                    config: serde_json::json!({ "threshold": 0.3 }),
                    retry_policy: None,
                    timeout_seconds: Some(10),
                    condition: None,
                },
                NodeDefinition {
                    id: "elevate_insight".to_string(),
                    node_type: NodeType::InsightExtract,
                    name: Some("Elevate to Insight".to_string()),
                    description: Some("Extract insight from conflicts".to_string()),
                    config: serde_json::json!({}),
                    retry_policy: None,
                    timeout_seconds: Some(120),
                    condition: Some("conflict_density > threshold".to_string()),
                },
            ],
            edges: vec![
                EdgeDefinition {
                    from: "fetch_claims".to_string(),
                    to: "detect_conflicts".to_string(),
                    edge_type: EdgeType::Sequential,
                    condition: None,
                },
                EdgeDefinition {
                    from: "detect_conflicts".to_string(),
                    to: "analyze_density".to_string(),
                    edge_type: EdgeType::Sequential,
                    condition: None,
                },
                EdgeDefinition {
                    from: "analyze_density".to_string(),
                    to: "elevate_insight".to_string(),
                    edge_type: EdgeType::Conditional,
                    condition: Some("conflict_density > threshold".to_string()),
                },
            ],
            variables: HashMap::new(),
            timeout_seconds: Some(300),
            retry_policy: Some(RetryPolicy::default()),
        }
    }

    /// Create a paradigm shift test workflow
    pub fn paradigm_shift_test() -> WorkflowDefinition {
        WorkflowDefinition {
            id: Uuid::new_v4().to_string(),
            name: "paradigm_shift_test".to_string(),
            version: "1.0".to_string(),
            description: Some("A/B test new framework against old".to_string()),
            nodes: vec![
                NodeDefinition {
                    id: "snapshot_state".to_string(),
                    node_type: NodeType::Task,
                    name: Some("Snapshot Current State".to_string()),
                    description: Some("Create backup before paradigm shift".to_string()),
                    config: serde_json::json!({ "action": "snapshot" }),
                    retry_policy: None,
                    timeout_seconds: Some(30),
                    condition: None,
                },
                NodeDefinition {
                    id: "ab_test".to_string(),
                    node_type: NodeType::AbTest,
                    name: Some("A/B Test Frameworks".to_string()),
                    description: Some("Compare old vs new framework".to_string()),
                    config: serde_json::json!({
                        "variants": ["old_framework", "new_framework"],
                        "duration_days": 7,
                        "success_metric": "prediction_accuracy",
                        "improvement_threshold": 0.10
                    }),
                    retry_policy: None,
                    timeout_seconds: Some(604800), // 7 days
                    condition: None,
                },
                NodeDefinition {
                    id: "evaluate".to_string(),
                    node_type: NodeType::Condition,
                    name: Some("Evaluate Results".to_string()),
                    description: Some("Check if new framework is better".to_string()),
                    config: serde_json::json!({}),
                    retry_policy: None,
                    timeout_seconds: Some(10),
                    condition: Some("new_framework_wins".to_string()),
                },
                NodeDefinition {
                    id: "switch".to_string(),
                    node_type: NodeType::Task,
                    name: Some("Switch Framework".to_string()),
                    description: Some("Atomically switch to new framework".to_string()),
                    config: serde_json::json!({ "action": "switch" }),
                    retry_policy: None,
                    timeout_seconds: Some(30),
                    condition: Some("new_framework_wins".to_string()),
                },
                NodeDefinition {
                    id: "rollback".to_string(),
                    node_type: NodeType::Task,
                    name: Some("Rollback".to_string()),
                    description: Some("Revert to old framework".to_string()),
                    config: serde_json::json!({ "action": "rollback" }),
                    retry_policy: None,
                    timeout_seconds: Some(30),
                    condition: Some("old_framework_wins".to_string()),
                },
            ],
            edges: vec![
                EdgeDefinition {
                    from: "snapshot_state".to_string(),
                    to: "ab_test".to_string(),
                    edge_type: EdgeType::Sequential,
                    condition: None,
                },
                EdgeDefinition {
                    from: "ab_test".to_string(),
                    to: "evaluate".to_string(),
                    edge_type: EdgeType::Sequential,
                    condition: None,
                },
                EdgeDefinition {
                    from: "evaluate".to_string(),
                    to: "switch".to_string(),
                    edge_type: EdgeType::Conditional,
                    condition: Some("new_framework_wins".to_string()),
                },
                EdgeDefinition {
                    from: "evaluate".to_string(),
                    to: "rollback".to_string(),
                    edge_type: EdgeType::Conditional,
                    condition: Some("old_framework_wins".to_string()),
                },
            ],
            variables: HashMap::new(),
            timeout_seconds: Some(604800),
            retry_policy: Some(RetryPolicy::default()),
        }
    }

    /// Create a knowledge consolidation workflow
    pub fn knowledge_consolidation() -> WorkflowDefinition {
        WorkflowDefinition {
            id: Uuid::new_v4().to_string(),
            name: "knowledge_consolidation".to_string(),
            version: "1.0".to_string(),
            description: Some(
                "Consolidate claims into beliefs using Bayesian aggregation".to_string(),
            ),
            nodes: vec![
                NodeDefinition {
                    id: "fetch_similar_claims".to_string(),
                    node_type: NodeType::Task,
                    name: Some("Fetch Similar Claims".to_string()),
                    description: Some("Find claims about the same topic".to_string()),
                    config: serde_json::json!({}),
                    retry_policy: None,
                    timeout_seconds: Some(30),
                    condition: None,
                },
                NodeDefinition {
                    id: "bayesian_aggregate".to_string(),
                    node_type: NodeType::Task,
                    name: Some("Bayesian Aggregation".to_string()),
                    description: Some("Aggregate claims using Bayesian inference".to_string()),
                    config: serde_json::json!({
                        "prior_strength": 1.0,
                        "min_confidence": 0.5
                    }),
                    retry_policy: None,
                    timeout_seconds: Some(60),
                    condition: None,
                },
                NodeDefinition {
                    id: "create_belief".to_string(),
                    node_type: NodeType::Task,
                    name: Some("Create Belief".to_string()),
                    description: Some("Create consolidated belief".to_string()),
                    config: serde_json::json!({}),
                    retry_policy: None,
                    timeout_seconds: Some(30),
                    condition: None,
                },
            ],
            edges: vec![
                EdgeDefinition {
                    from: "fetch_similar_claims".to_string(),
                    to: "bayesian_aggregate".to_string(),
                    edge_type: EdgeType::Sequential,
                    condition: None,
                },
                EdgeDefinition {
                    from: "bayesian_aggregate".to_string(),
                    to: "create_belief".to_string(),
                    edge_type: EdgeType::Sequential,
                    condition: None,
                },
            ],
            variables: HashMap::new(),
            timeout_seconds: Some(180),
            retry_policy: Some(RetryPolicy::default()),
        }
    }
}
