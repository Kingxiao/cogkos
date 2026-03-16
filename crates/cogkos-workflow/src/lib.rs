//! CogKOS Workflow Orchestration
//!
//! **STATUS: FROZEN** - This crate is not active in V1.
//! Workflow engine and A/B testing are deferred to V2/V3.
//! Code is preserved for future use but not initialized at runtime.
//!
//! This crate provides:
//! - Workflow Engine: Execute and manage complex workflows
//! - A/B Testing Framework: Compare different strategies and approaches
//! - Workflow DSL: Define workflows in a declarative way

pub mod ab_testing;
pub mod dsl;
pub mod engine;

pub use ab_testing::{AbTestFramework, TestResult, TestVariant};
pub use dsl::{EdgeDefinition, NodeDefinition, WorkflowDefinition, WorkflowParser};
pub use engine::{
    EdgeType, ExecutionContext, NodeType, WorkflowEngine, WorkflowExecutor, WorkflowNode,
    WorkflowState,
};

use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum WorkflowError {
    #[error("Workflow not found: {0}")]
    WorkflowNotFound(String),

    #[error("Node not found: {0}")]
    NodeNotFound(String),

    #[error("Invalid workflow definition: {0}")]
    InvalidDefinition(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("A/B test error: {0}")]
    AbTestError(String),

    #[error("DSL parsing error: {0}")]
    DslError(String),

    #[error("State transition error: from {from} to {to}")]
    StateTransition { from: String, to: String },
}

pub type Result<T> = std::result::Result<T, WorkflowError>;

/// Workflow execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExecutionStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Paused,
}

/// Node execution result
#[derive(Debug, Clone)]
pub struct NodeResult {
    pub node_id: String,
    pub success: bool,
    pub output: serde_json::Value,
    pub duration_ms: u64,
    pub error: Option<String>,
}
