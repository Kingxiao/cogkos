use crate::{Result, WorkflowError};
use petgraph::graph::{DiGraph, NodeIndex};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Workflow definition in declarative format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub nodes: Vec<NodeDefinition>,
    pub edges: Vec<EdgeDefinition>,
    pub variables: HashMap<String, VariableDefinition>,
    pub timeout_seconds: Option<u64>,
    pub retry_policy: Option<RetryPolicy>,
}

/// Node definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDefinition {
    pub id: String,
    pub node_type: NodeType,
    pub name: Option<String>,
    pub description: Option<String>,
    pub config: Value,
    pub retry_policy: Option<RetryPolicy>,
    pub timeout_seconds: Option<u64>,
    pub condition: Option<String>, // For conditional nodes
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Task,
    Condition,
    Parallel,
    Map,
    Reduce,
    Wait,
    SubWorkflow,
    AbTest,
    InsightExtract,
    ConflictDetect,
}

/// Edge definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeDefinition {
    pub from: String,
    pub to: String,
    pub edge_type: EdgeType,
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    Sequential,
    Conditional,
    Error,
    Parallel,
}

/// Variable definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableDefinition {
    pub var_type: VariableType,
    pub default: Option<Value>,
    pub required: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableType {
    String,
    Number,
    Boolean,
    Array,
    Object,
}

/// Retry policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff_multiplier: f64,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub retryable_errors: Vec<String>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff_multiplier: 2.0,
            initial_delay_ms: 1000,
            max_delay_ms: 60000,
            retryable_errors: vec!["timeout".to_string(), "transient".to_string()],
        }
    }
}

/// Workflow DSL parser
pub struct WorkflowParser;

impl WorkflowParser {
    pub fn new() -> Self {
        Self
    }

    /// Parse workflow from JSON
    pub fn parse_json(&self, json: &str) -> Result<WorkflowDefinition> {
        let workflow: WorkflowDefinition = serde_json::from_str(json)
            .map_err(|e| WorkflowError::DslError(format!("JSON parse error: {}", e)))?;

        self.validate(&workflow)?;
        Ok(workflow)
    }

    /// Parse workflow from YAML
    pub fn parse_yaml(&self, yaml: &str) -> Result<WorkflowDefinition> {
        let workflow: WorkflowDefinition = serde_yaml::from_str(yaml)
            .map_err(|e| WorkflowError::DslError(format!("YAML parse error: {}", e)))?;

        self.validate(&workflow)?;
        Ok(workflow)
    }

    /// Parse workflow from CogKOS DSL (custom format)
    pub fn parse_dsl(&self, dsl: &str) -> Result<WorkflowDefinition> {
        // Custom DSL parser for simplified workflow definitions
        // Format example:
        // workflow "insight_extraction" {
        //   node "detect_conflicts" { type: task }
        //   node "analyze_patterns" { type: task }
        //   edge "detect_conflicts" -> "analyze_patterns"
        // }

        let mut workflow = WorkflowDefinition {
            id: Uuid::new_v4().to_string(),
            name: "parsed_workflow".to_string(),
            version: "1.0".to_string(),
            description: None,
            nodes: Vec::new(),
            edges: Vec::new(),
            variables: HashMap::new(),
            timeout_seconds: None,
            retry_policy: None,
        };

        // Simple regex-based parser
        let workflow_re = Regex::new(r#"workflow\s+"([^"]+)"\s*\{"#)
            .map_err(|e| WorkflowError::DslError(e.to_string()))?;

        let node_re = Regex::new(r#"node\s+"([^"]+)"\s*\{\s*type:\s*(\w+)"#)
            .map_err(|e| WorkflowError::DslError(e.to_string()))?;

        let edge_re = Regex::new(r#"edge\s+"([^"]+)"\s*->\s*"([^"]+)""#)
            .map_err(|e| WorkflowError::DslError(e.to_string()))?;

        // Extract workflow name
        if let Some(cap) = workflow_re.captures(dsl) {
            workflow.name = cap[1].to_string();
        }

        // Extract nodes
        for cap in node_re.captures_iter(dsl) {
            let node_type = match &cap[2] {
                "task" => NodeType::Task,
                "condition" => NodeType::Condition,
                "parallel" => NodeType::Parallel,
                "wait" => NodeType::Wait,
                "ab_test" => NodeType::AbTest,
                "insight" => NodeType::InsightExtract,
                "conflict" => NodeType::ConflictDetect,
                _ => NodeType::Task,
            };

            workflow.nodes.push(NodeDefinition {
                id: cap[1].to_string(),
                node_type,
                name: Some(cap[1].to_string()),
                description: None,
                config: Value::Object(Default::default()),
                retry_policy: None,
                timeout_seconds: None,
                condition: None,
            });
        }

        // Extract edges
        for cap in edge_re.captures_iter(dsl) {
            workflow.edges.push(EdgeDefinition {
                from: cap[1].to_string(),
                to: cap[2].to_string(),
                edge_type: EdgeType::Sequential,
                condition: None,
            });
        }

        self.validate(&workflow)?;
        Ok(workflow)
    }

    /// Validate workflow definition
    pub fn validate(&self, workflow: &WorkflowDefinition) -> Result<()> {
        // Check for duplicate node IDs
        let mut seen_ids = std::collections::HashSet::new();
        for node in &workflow.nodes {
            if !seen_ids.insert(&node.id) {
                return Err(WorkflowError::InvalidDefinition(format!(
                    "Duplicate node ID: {}",
                    node.id
                )));
            }
        }

        // Validate edges reference existing nodes
        let node_ids: std::collections::HashSet<_> = workflow.nodes.iter().map(|n| &n.id).collect();

        for edge in &workflow.edges {
            if !node_ids.contains(&edge.from) {
                return Err(WorkflowError::InvalidDefinition(format!(
                    "Edge references non-existent node: {}",
                    edge.from
                )));
            }
            if !node_ids.contains(&edge.to) {
                return Err(WorkflowError::InvalidDefinition(format!(
                    "Edge references non-existent node: {}",
                    edge.to
                )));
            }
        }

        // Check for cycles (simple check)
        let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();
        for edge in &workflow.edges {
            graph.entry(&edge.from).or_default().push(&edge.to);
        }

        // Find start nodes (no incoming edges)
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        for node in &workflow.nodes {
            in_degree.entry(&node.id).or_insert(0);
        }
        for edge in &workflow.edges {
            *in_degree.entry(&edge.to).or_insert(0) += 1;
        }

        // Topological sort
        let mut queue: Vec<_> = in_degree
            .iter()
            .filter(|&(_, &d)| d == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut processed = 0;
        while let Some(node) = queue.pop() {
            processed += 1;
            if let Some(neighbors) = graph.get(node) {
                for &neighbor in neighbors {
                    let deg = in_degree.get_mut(neighbor).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(neighbor);
                    }
                }
            }
        }

        if processed != workflow.nodes.len() {
            return Err(WorkflowError::InvalidDefinition(
                "Workflow contains cycles".to_string(),
            ));
        }

        Ok(())
    }

    /// Convert WorkflowDefinition to petgraph for execution
    pub fn to_graph(
        &self,
        workflow: &WorkflowDefinition,
    ) -> Result<DiGraph<ParsedNode, ParsedEdge>> {
        let mut graph = DiGraph::new();
        let mut node_indices: HashMap<String, NodeIndex> = HashMap::new();

        // Add nodes
        for node_def in &workflow.nodes {
            let node = ParsedNode {
                id: node_def.id.clone(),
                node_type: node_def.node_type,
                config: node_def.config.clone(),
                condition: node_def.condition.clone(),
            };
            let idx = graph.add_node(node);
            node_indices.insert(node_def.id.clone(), idx);
        }

        // Add edges
        for edge_def in &workflow.edges {
            let from_idx = node_indices.get(&edge_def.from).ok_or_else(|| {
                WorkflowError::InvalidDefinition(format!("Node not found: {}", edge_def.from))
            })?;
            let to_idx = node_indices.get(&edge_def.to).ok_or_else(|| {
                WorkflowError::InvalidDefinition(format!("Node not found: {}", edge_def.to))
            })?;

            let edge = ParsedEdge {
                edge_type: edge_def.edge_type,
                condition: edge_def.condition.clone(),
            };

            graph.add_edge(*from_idx, *to_idx, edge);
        }

        Ok(graph)
    }

    /// Serialize workflow to JSON
    pub fn to_json(&self, workflow: &WorkflowDefinition) -> Result<String> {
        serde_json::to_string_pretty(workflow)
            .map_err(|e| WorkflowError::DslError(format!("Serialization error: {}", e)))
    }

    /// Serialize workflow to YAML
    pub fn to_yaml(&self, workflow: &WorkflowDefinition) -> Result<String> {
        serde_yaml::to_string(workflow)
            .map_err(|e| WorkflowError::DslError(format!("Serialization error: {}", e)))
    }
}

impl Default for WorkflowParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Parsed node for graph representation
#[derive(Debug, Clone)]
pub struct ParsedNode {
    pub id: String,
    pub node_type: NodeType,
    pub config: Value,
    pub condition: Option<String>,
}

/// Parsed edge for graph representation
#[derive(Debug, Clone)]
pub struct ParsedEdge {
    pub edge_type: EdgeType,
    pub condition: Option<String>,
}

use uuid::Uuid;
