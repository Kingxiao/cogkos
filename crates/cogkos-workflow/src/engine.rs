use crate::{ExecutionStatus, NodeResult, Result, WorkflowError};
use cogkos_core::evolution::decay::{
    calculate_decay, calculate_decay_with_revalidation, needs_revalidation,
};
use dashmap::DashMap;
use futures::future::BoxFuture;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Workflow state machine
#[derive(Debug, Clone)]
pub struct WorkflowState {
    pub workflow_id: String,
    pub status: ExecutionStatus,
    pub current_nodes: HashSet<String>,
    pub completed_nodes: HashSet<String>,
    pub failed_nodes: HashSet<String>,
    pub variables: HashMap<String, Value>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl WorkflowState {
    pub fn new(workflow_id: String) -> Self {
        Self {
            workflow_id,
            status: ExecutionStatus::Pending,
            current_nodes: HashSet::new(),
            completed_nodes: HashSet::new(),
            failed_nodes: HashSet::new(),
            variables: HashMap::new(),
            started_at: chrono::Utc::now(),
            completed_at: None,
        }
    }

    pub fn is_complete(&self, total_nodes: usize) -> bool {
        self.completed_nodes.len() + self.failed_nodes.len() >= total_nodes
    }

    pub fn get_variable(&self, name: &str) -> Option<&Value> {
        self.variables.get(name)
    }

    pub fn set_variable(&mut self, name: String, value: Value) {
        self.variables.insert(name, value);
    }
}

/// Execution context passed to node handlers
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub workflow_id: String,
    pub node_id: String,
    pub variables: HashMap<String, Value>,
    pub inputs: HashMap<String, Value>,
}

impl ExecutionContext {
    pub fn get_input(&self, name: &str) -> Option<&Value> {
        self.inputs.get(name)
    }

    pub fn get_variable(&self, name: &str) -> Option<&Value> {
        self.variables.get(name)
    }
}

/// Node handler trait for custom node execution
#[async_trait::async_trait]
pub trait NodeHandler: Send + Sync {
    async fn execute(&self, ctx: ExecutionContext) -> Result<NodeResult>;
}

type HandlerFn =
    Arc<dyn Fn(ExecutionContext) -> BoxFuture<'static, Result<NodeResult>> + Send + Sync>;

/// Node definition for the workflow engine
#[derive(Clone)]
pub struct WorkflowNode {
    pub id: String,
    pub node_type: NodeType,
    pub config: Value,
    pub handler: Option<HandlerFn>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NodeType {
    Task,
    Condition,
    Parallel,
    Map,
    Reduce,
    Wait,
    SubWorkflow,
    // Evolution engine nodes
    BayesianAggregate,
    KnowledgeDecay,
    ConflictDetection,
    Revalidation,
}

/// Workflow definition for engine execution
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub graph: DiGraph<WorkflowNode, EdgeType>,
    pub start_node: NodeIndex,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EdgeType {
    Sequential,
    Conditional { condition: Condition },
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Condition {
    Expression(String),
    Always,
    Never,
}

/// Workflow execution engine
pub struct WorkflowEngine {
    workflows: Arc<DashMap<String, Workflow>>,
    executions: Arc<DashMap<String, Arc<RwLock<WorkflowState>>>>,
    handlers: Arc<DashMap<String, HandlerFn>>,
    command_tx: mpsc::UnboundedSender<EngineCommand>,
}

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
enum EngineCommand {
    StartExecution {
        workflow_id: String,
        execution_id: String,
        initial_vars: HashMap<String, Value>,
    },
    CancelExecution {
        execution_id: String,
    },
    PauseExecution {
        execution_id: String,
    },
    ResumeExecution {
        execution_id: String,
    },
}

impl WorkflowEngine {
    pub fn new() -> Self {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let engine = Self {
            workflows: Arc::new(DashMap::new()),
            executions: Arc::new(DashMap::new()),
            handlers: Arc::new(DashMap::new()),
            command_tx,
        };

        // Spawn command processing loop
        let workflows = engine.workflows.clone();
        let executions = engine.executions.clone();
        tokio::spawn(async move {
            while let Some(cmd) = command_rx.recv().await {
                match cmd {
                    EngineCommand::StartExecution {
                        workflow_id,
                        execution_id,
                        initial_vars,
                    } => {
                        if let Some(_workflow) = workflows.get(&workflow_id) {
                            let mut state = WorkflowState::new(execution_id.clone());
                            state.variables = initial_vars;
                            state.status = ExecutionStatus::Running;
                            executions.insert(execution_id.clone(), Arc::new(RwLock::new(state)));
                            info!("Started workflow execution: {}", execution_id);
                        } else {
                            error!("Workflow not found: {}", workflow_id);
                        }
                    }
                    EngineCommand::CancelExecution { execution_id } => {
                        if let Some(exec) = executions.get(&execution_id) {
                            let mut state = exec.write().await;
                            state.status = ExecutionStatus::Cancelled;
                            info!("Cancelled workflow execution: {}", execution_id);
                        }
                    }
                    EngineCommand::PauseExecution { execution_id } => {
                        if let Some(exec) = executions.get(&execution_id) {
                            let mut state = exec.write().await;
                            if state.status == ExecutionStatus::Running {
                                state.status = ExecutionStatus::Paused;
                                info!("Paused workflow execution: {}", execution_id);
                            }
                        }
                    }
                    EngineCommand::ResumeExecution { execution_id } => {
                        if let Some(exec) = executions.get(&execution_id) {
                            let mut state = exec.write().await;
                            if state.status == ExecutionStatus::Paused {
                                state.status = ExecutionStatus::Running;
                                info!("Resumed workflow execution: {}", execution_id);
                            }
                        }
                    }
                }
            }
        });

        engine
    }

    /// Register a workflow
    pub fn register_workflow(&self, workflow: Workflow) {
        self.workflows.insert(workflow.id.clone(), workflow);
    }

    /// Register a handler for a node type
    pub fn register_handler<F>(&self, node_type: &str, handler: F)
    where
        F: Fn(ExecutionContext) -> BoxFuture<'static, Result<NodeResult>> + Send + Sync + 'static,
    {
        self.handlers
            .insert(node_type.to_string(), Arc::new(handler));
    }

    /// Start a new workflow execution
    pub fn start_execution(
        &self,
        workflow_id: &str,
        initial_vars: HashMap<String, Value>,
    ) -> Result<String> {
        if !self.workflows.contains_key(workflow_id) {
            return Err(WorkflowError::WorkflowNotFound(workflow_id.to_string()));
        }

        let execution_id = Uuid::new_v4().to_string();
        self.command_tx
            .send(EngineCommand::StartExecution {
                workflow_id: workflow_id.to_string(),
                execution_id: execution_id.clone(),
                initial_vars,
            })
            .map_err(|_| WorkflowError::ExecutionError("Failed to send command".to_string()))?;

        Ok(execution_id)
    }

    /// Get execution state
    pub async fn get_execution_state(&self, execution_id: &str) -> Option<WorkflowState> {
        let exec = self.executions.get(execution_id)?;
        Some(exec.read().await.clone())
    }

    /// Cancel an execution
    pub fn cancel_execution(&self, execution_id: &str) -> Result<()> {
        self.command_tx
            .send(EngineCommand::CancelExecution {
                execution_id: execution_id.to_string(),
            })
            .map_err(|_| WorkflowError::ExecutionError("Failed to send command".to_string()))
    }

    /// Pause an execution
    pub fn pause_execution(&self, execution_id: &str) -> Result<()> {
        self.command_tx
            .send(EngineCommand::PauseExecution {
                execution_id: execution_id.to_string(),
            })
            .map_err(|_| WorkflowError::ExecutionError("Failed to send command".to_string()))
    }

    /// Resume an execution
    pub fn resume_execution(&self, execution_id: &str) -> Result<()> {
        self.command_tx
            .send(EngineCommand::ResumeExecution {
                execution_id: execution_id.to_string(),
            })
            .map_err(|_| WorkflowError::ExecutionError("Failed to send command".to_string()))
    }
}

impl Default for WorkflowEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Workflow executor - handles actual node execution
pub struct WorkflowExecutor {
    engine: Arc<WorkflowEngine>,
}

impl WorkflowExecutor {
    pub fn new(engine: Arc<WorkflowEngine>) -> Self {
        Self { engine }
    }

    pub async fn execute_workflow(&self, execution_id: &str) -> Result<WorkflowState> {
        let execution = self
            .engine
            .executions
            .get(execution_id)
            .ok_or_else(|| WorkflowError::ExecutionError("Execution not found".to_string()))?;

        // Get workflow ID from state
        let workflow_id = {
            let state = execution.read().await;
            state.workflow_id.clone()
        };

        let workflow = self
            .engine
            .workflows
            .get(&workflow_id)
            .ok_or_else(|| WorkflowError::WorkflowNotFound(workflow_id.clone()))?;

        // BFS execution
        let mut queue = VecDeque::new();
        queue.push_back(workflow.start_node);

        while let Some(node_idx) = queue.pop_front() {
            // Check if execution is still running
            {
                let state = execution.read().await;
                if state.status != ExecutionStatus::Running {
                    break;
                }
            }

            let node = &workflow.graph[node_idx];

            // Execute node
            let result = self.execute_node(&workflow_id, execution_id, node).await;

            match result {
                Ok(node_result) => {
                    let mut state = execution.write().await;
                    state.completed_nodes.insert(node.id.clone());

                    // Store output in variables
                    if !node_result.output.is_null() {
                        state.set_variable(format!("output.{}", node.id), node_result.output);
                    }

                    // Add next nodes to queue
                    for edge in workflow.graph.edges(node_idx) {
                        let target = edge.target();
                        let edge_type = edge.weight();

                        match edge_type {
                            EdgeType::Sequential => {
                                if !state.completed_nodes.contains(&workflow.graph[target].id) {
                                    queue.push_back(target);
                                }
                            }
                            EdgeType::Conditional { condition } => {
                                if self.evaluate_condition(condition, &state) {
                                    queue.push_back(target);
                                }
                            }
                            EdgeType::Error => {}
                        }
                    }
                }
                Err(e) => {
                    let mut state = execution.write().await;
                    state.failed_nodes.insert(node.id.clone());
                    state.status = ExecutionStatus::Failed;
                    error!("Node execution failed: {:?}", e);
                    break;
                }
            }
        }

        // Mark as completed if all nodes processed
        let mut state = execution.write().await;
        if state.status == ExecutionStatus::Running {
            state.status = ExecutionStatus::Completed;
            state.completed_at = Some(chrono::Utc::now());
        }

        Ok(state.clone())
    }

    async fn execute_node(
        &self,
        _workflow_id: &str,
        execution_id: &str,
        node: &WorkflowNode,
    ) -> Result<NodeResult> {
        let start = std::time::Instant::now();

        // Get execution context
        let ctx = self.build_context(execution_id, node).await?;

        // Execute based on node type
        let result = match &node.node_type {
            NodeType::Task => {
                if let Some(handler) = node.handler.as_ref() {
                    handler(ctx).await
                } else if let Some(handler) = self.engine.handlers.get("task") {
                    handler(ctx).await
                } else {
                    Err(WorkflowError::ExecutionError(
                        "No handler for task node".to_string(),
                    ))
                }
            }
            NodeType::Condition => self.execute_condition_node(ctx, node).await,
            NodeType::Parallel => self.execute_parallel_node(ctx, node).await,
            NodeType::Wait => self.execute_wait_node(ctx, node).await,
            NodeType::Map => self.execute_map_node(ctx, node).await,
            NodeType::Reduce => self.execute_reduce_node(ctx, node).await,
            NodeType::SubWorkflow => self.execute_subworkflow_node(ctx, node).await,
            // Evolution engine nodes
            NodeType::BayesianAggregate => self.execute_bayesian_aggregate_node(ctx, node).await,
            NodeType::KnowledgeDecay => self.execute_knowledge_decay_node(ctx, node).await,
            NodeType::ConflictDetection => self.execute_conflict_detection_node(ctx, node).await,
            NodeType::Revalidation => self.execute_revalidation_node(ctx, node).await,
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(mut r) => {
                r.duration_ms = duration_ms;
                Ok(r)
            }
            Err(e) => Ok(NodeResult {
                node_id: node.id.clone(),
                success: false,
                output: Value::Null,
                duration_ms,
                error: Some(e.to_string()),
            }),
        }
    }

    async fn build_context(
        &self,
        execution_id: &str,
        node: &WorkflowNode,
    ) -> Result<ExecutionContext> {
        let execution = self
            .engine
            .executions
            .get(execution_id)
            .ok_or_else(|| WorkflowError::ExecutionError("Execution not found".to_string()))?;

        let state = execution.read().await;

        Ok(ExecutionContext {
            workflow_id: state.workflow_id.clone(),
            node_id: node.id.clone(),
            variables: state.variables.clone(),
            inputs: node
                .config
                .clone()
                .as_object()
                .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default(),
        })
    }

    async fn execute_condition_node(
        &self,
        ctx: ExecutionContext,
        node: &WorkflowNode,
    ) -> Result<NodeResult> {
        let condition = node
            .config
            .get("condition")
            .and_then(|c| c.as_str())
            .ok_or_else(|| WorkflowError::ExecutionError("Missing condition".to_string()))?;

        let result = self.evaluate_simple_condition(condition, &ctx);

        Ok(NodeResult {
            node_id: node.id.clone(),
            success: true,
            output: serde_json::json!({ "result": result }),
            duration_ms: 0,
            error: None,
        })
    }

    async fn execute_parallel_node(
        &self,
        ctx: ExecutionContext,
        node: &WorkflowNode,
    ) -> Result<NodeResult> {
        let branches = node
            .config
            .get("branches")
            .and_then(|b| b.as_array())
            .ok_or_else(|| WorkflowError::ExecutionError("Missing branches".to_string()))?;

        let mut handles = vec![];

        for (i, _branch) in branches.iter().enumerate() {
            let _ctx_clone = ctx.clone();
            let branch_id = format!("{}_branch_{}", node.id, i);

            let handle = tokio::spawn(async move {
                // Simulate branch execution
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                serde_json::json!({ "branch_id": branch_id, "result": "completed" })
            });

            handles.push(handle);
        }

        let results: Vec<_> = futures::future::join_all(handles)
            .await
            .into_iter()
            .filter_map(|r| r.ok())
            .collect();

        Ok(NodeResult {
            node_id: node.id.clone(),
            success: true,
            output: serde_json::json!({ "branches": results }),
            duration_ms: 0,
            error: None,
        })
    }

    async fn execute_wait_node(
        &self,
        _ctx: ExecutionContext,
        node: &WorkflowNode,
    ) -> Result<NodeResult> {
        let duration_ms = node
            .config
            .get("duration_ms")
            .and_then(|d| d.as_u64())
            .unwrap_or(1000);

        tokio::time::sleep(tokio::time::Duration::from_millis(duration_ms)).await;

        Ok(NodeResult {
            node_id: node.id.clone(),
            success: true,
            output: serde_json::json!({ "waited_ms": duration_ms }),
            duration_ms,
            error: None,
        })
    }

    /// Execute a Map node - applies operation to each element in a collection
    async fn execute_map_node(
        &self,
        ctx: ExecutionContext,
        node: &WorkflowNode,
    ) -> Result<NodeResult> {
        // Get the collection to iterate over
        let collection_key = node
            .config
            .get("collection")
            .and_then(|c| c.as_str())
            .ok_or_else(|| WorkflowError::ExecutionError("Missing collection key".to_string()))?;

        let collection = ctx
            .variables
            .get(collection_key)
            .or_else(|| ctx.inputs.get(collection_key))
            .and_then(|v| v.as_array())
            .cloned()
            .ok_or_else(|| {
                WorkflowError::ExecutionError(format!("Collection not found: {}", collection_key))
            })?;

        // Get the operation to apply (handler name or inline expression)
        let operation = node
            .config
            .get("operation")
            .and_then(|o| o.as_str())
            .unwrap_or("map");

        // Get variable name to store results
        let output_key = node
            .config
            .get("output")
            .and_then(|o| o.as_str())
            .unwrap_or("map_result");

        let mut results = Vec::new();

        // Execute operation for each element
        for (i, item) in collection.iter().enumerate() {
            let item_ctx = ExecutionContext {
                workflow_id: ctx.workflow_id.clone(),
                node_id: format!("{}_item_{}", node.id, i),
                variables: ctx.variables.clone(),
                inputs: [
                    ("item".to_string(), item.clone()),
                    ("index".to_string(), serde_json::json!(i)),
                    ("operation".to_string(), serde_json::json!(operation)),
                ]
                .into_iter()
                .collect(),
            };

            // Try custom handler first, then built-in operations
            let result = if let Some(handler) = node.handler.as_ref() {
                handler(item_ctx).await
            } else if let Some(handler) = self.engine.handlers.get(operation) {
                handler(item_ctx).await
            } else {
                // Built-in map operations
                let item_val = item_ctx.get_input("item").cloned().unwrap_or(Value::Null);
                Ok(NodeResult {
                    node_id: item_ctx.node_id.clone(),
                    success: true,
                    output: item_val,
                    duration_ms: 0,
                    error: None,
                })
            };

            match result {
                Ok(r) => results.push(r.output),
                Err(e) => {
                    return Err(WorkflowError::ExecutionError(format!(
                        "Map operation failed at index {}: {}",
                        i, e
                    )));
                }
            }
        }

        let output = serde_json::json!({
            output_key: results,
            "count": results.len()
        });

        Ok(NodeResult {
            node_id: node.id.clone(),
            success: true,
            output,
            duration_ms: 0,
            error: None,
        })
    }

    /// Execute a Reduce node - aggregates a collection into a single value
    async fn execute_reduce_node(
        &self,
        ctx: ExecutionContext,
        node: &WorkflowNode,
    ) -> Result<NodeResult> {
        // Get the collection to reduce
        let collection_key = node
            .config
            .get("collection")
            .and_then(|c| c.as_str())
            .ok_or_else(|| WorkflowError::ExecutionError("Missing collection key".to_string()))?;

        let collection = ctx
            .variables
            .get(collection_key)
            .or_else(|| ctx.inputs.get(collection_key))
            .and_then(|v| v.as_array())
            .cloned()
            .ok_or_else(|| {
                WorkflowError::ExecutionError(format!("Collection not found: {}", collection_key))
            })?;

        // Get the reduction operation
        let operation = node
            .config
            .get("operation")
            .and_then(|o| o.as_str())
            .unwrap_or("sum");

        // Get initial value if specified
        let initial_value = node.config.get("initial").cloned();

        // Get variable name to store result
        let output_key = node
            .config
            .get("output")
            .and_then(|o| o.as_str())
            .unwrap_or("reduce_result");

        // Perform reduction
        let result = match operation {
            "sum" => {
                let sum: f64 = collection.iter().filter_map(|v| v.as_f64()).sum();
                serde_json::json!(sum)
            }
            "avg" | "average" => {
                let sum: f64 = collection.iter().filter_map(|v| v.as_f64()).sum();
                let count = collection.iter().filter_map(|v| v.as_f64()).count();
                if count > 0 {
                    serde_json::json!(sum / count as f64)
                } else {
                    serde_json::json!(0)
                }
            }
            "min" => collection
                .iter()
                .filter_map(|v| v.as_f64())
                .reduce(f64::min)
                .map(|v| serde_json::json!(v))
                .unwrap_or(serde_json::json!(null)),
            "max" => collection
                .iter()
                .filter_map(|v| v.as_f64())
                .reduce(f64::max)
                .map(|v| serde_json::json!(v))
                .unwrap_or(serde_json::json!(null)),
            "count" => serde_json::json!(collection.len()),
            "collect" => serde_json::json!(collection),
            "merge" | "concat" => {
                let mut strings = Vec::new();
                for item in &collection {
                    if let Some(s) = item.as_str() {
                        strings.push(s.to_string());
                    } else if let Some(arr) = item.as_array() {
                        for v in arr {
                            if let Some(s) = v.as_str() {
                                strings.push(s.to_string());
                            }
                        }
                    }
                }
                serde_json::json!(strings.join(""))
            }
            "custom" => {
                // Custom reduction via handler
                let mut accumulator = initial_value.unwrap_or(Value::Null);

                for (i, item) in collection.iter().enumerate() {
                    let reduce_ctx = ExecutionContext {
                        workflow_id: ctx.workflow_id.clone(),
                        node_id: format!("{}_reduce_{}", node.id, i),
                        variables: ctx.variables.clone(),
                        inputs: [
                            ("accumulator".to_string(), accumulator),
                            ("item".to_string(), item.clone()),
                            ("index".to_string(), serde_json::json!(i)),
                        ]
                        .into_iter()
                        .collect(),
                    };

                    if let Some(handler) = node.handler.as_ref() {
                        accumulator = handler(reduce_ctx).await?.output;
                    } else {
                        return Err(WorkflowError::ExecutionError(
                            "Custom reduce requires a handler".to_string(),
                        ));
                    }
                }
                accumulator
            }
            _ => {
                return Err(WorkflowError::ExecutionError(format!(
                    "Unknown reduce operation: {}",
                    operation
                )));
            }
        };

        let output = serde_json::json!({
            output_key: result,
            "operation": operation,
            "count": collection.len()
        });

        Ok(NodeResult {
            node_id: node.id.clone(),
            success: true,
            output,
            duration_ms: 0,
            error: None,
        })
    }

    /// Execute a SubWorkflow node - invokes a child workflow
    async fn execute_subworkflow_node(
        &self,
        ctx: ExecutionContext,
        node: &WorkflowNode,
    ) -> Result<NodeResult> {
        // Get the sub-workflow ID to execute
        let workflow_id = node
            .config
            .get("workflow_id")
            .and_then(|w| w.as_str())
            .ok_or_else(|| WorkflowError::ExecutionError("Missing workflow_id".to_string()))?;

        // Get input mappings (how to pass data to sub-workflow)
        let input_mapping = node
            .config
            .get("inputs")
            .and_then(|i| i.as_object())
            .map(|o| {
                o.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<HashMap<String, Value>>()
            })
            .unwrap_or_default();

        // Build input variables for sub-workflow
        let mut sub_vars = HashMap::new();
        for (target_key, source_expr) in input_mapping {
            // Simple variable reference resolution
            let source_key = source_expr
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| source_expr.to_string());
            if let Some(source_val) = ctx.variables.get(&source_key) {
                sub_vars.insert(target_key.clone(), source_val.clone());
            } else if let Some(source_val) = ctx.inputs.get(&source_key) {
                sub_vars.insert(target_key.clone(), source_val.clone());
            } else {
                // Use literal value
                sub_vars.insert(target_key.clone(), source_expr.clone());
            }
        }

        // Start sub-workflow execution
        let execution_id = self.engine.start_execution(workflow_id, sub_vars)?;

        // Wait for sub-workflow to complete (with timeout)
        let timeout_ms = node
            .config
            .get("timeout_ms")
            .and_then(|t| t.as_u64())
            .unwrap_or(30000); // Default 30s timeout

        let result = tokio::time::timeout(tokio::time::Duration::from_millis(timeout_ms), async {
            loop {
                if let Some(state) = self.engine.get_execution_state(&execution_id).await {
                    match state.status {
                        ExecutionStatus::Completed => {
                            break Ok(state);
                        }
                        ExecutionStatus::Failed | ExecutionStatus::Cancelled => {
                            let status_str = match state.status {
                                ExecutionStatus::Pending => "pending",
                                ExecutionStatus::Running => "running",
                                ExecutionStatus::Completed => "completed",
                                ExecutionStatus::Failed => "failed",
                                ExecutionStatus::Cancelled => "cancelled",
                                ExecutionStatus::Paused => "paused",
                            };
                            break Err(WorkflowError::ExecutionError(format!(
                                "Sub-workflow {} {}",
                                status_str, execution_id
                            )));
                        }
                        _ => {
                            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                        }
                    }
                } else {
                    break Err(WorkflowError::ExecutionError(format!(
                        "Sub-workflow execution not found: {}",
                        execution_id
                    )));
                }
            }
        })
        .await;

        match result {
            Ok(Ok(state)) => {
                let output_key = node
                    .config
                    .get("output")
                    .and_then(|o| o.as_str())
                    .unwrap_or("subworkflow_result");

                let output = serde_json::json!({
                    output_key: {
                        "execution_id": execution_id,
                        "status": match state.status {
                            ExecutionStatus::Pending => "pending",
                            ExecutionStatus::Running => "running",
                            ExecutionStatus::Completed => "completed",
                            ExecutionStatus::Failed => "failed",
                            ExecutionStatus::Cancelled => "cancelled",
                            ExecutionStatus::Paused => "paused",
                        },
                        "variables": state.variables
                    }
                });

                Ok(NodeResult {
                    node_id: node.id.clone(),
                    success: state.status == ExecutionStatus::Completed,
                    output,
                    duration_ms: 0,
                    error: None,
                })
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WorkflowError::ExecutionError(format!(
                "Sub-workflow execution timeout after {}ms",
                timeout_ms
            ))),
        }
    }

    // ==================== Evolution Engine Nodes ====================

    /// Execute a BayesianAggregate node - combines multiple claims into a single belief
    async fn execute_bayesian_aggregate_node(
        &self,
        ctx: ExecutionContext,
        node: &WorkflowNode,
    ) -> Result<NodeResult> {
        // Get claims from input variable
        let claims_key = node
            .config
            .get("claims")
            .and_then(|c| c.as_str())
            .unwrap_or("claims");

        let claims_array = ctx
            .variables
            .get(claims_key)
            .or_else(|| ctx.inputs.get(claims_key))
            .and_then(|v| v.as_array())
            .cloned()
            .ok_or_else(|| {
                WorkflowError::ExecutionError(format!("Claims not found: {}", claims_key))
            })?;

        // Check if deduplication is enabled
        let deduplicate = node
            .config
            .get("deduplicate")
            .and_then(|d| d.as_bool())
            .unwrap_or(true);

        // Perform Bayesian aggregation
        let aggregated_confidence = if deduplicate {
            // Convert JSON claims to EpistemicClaim (simplified for workflow)
            // In production, this would deserialize properly from the store
            warn!(
                "Bayesian aggregate with deduplication requires full EpistemicClaim deserialization"
            );
            0.5 // Placeholder - requires full model
        } else {
            // For now, extract confidence values from claims array
            let confidences: Vec<f64> = claims_array
                .iter()
                .filter_map(|c| c.get("confidence").and_then(|v| v.as_f64()))
                .collect();

            if confidences.is_empty() {
                return Err(WorkflowError::ExecutionError(
                    "No valid confidence values found in claims".to_string(),
                ));
            }

            // Simple average for now - full Bayesian would use log_odds
            // This is a simplified implementation for workflow integration
            let sum: f64 = confidences.iter().sum();
            let avg = sum / confidences.len() as f64;

            // Boost confidence when multiple sources agree
            if confidences.len() > 1 {
                (avg + 0.1).min(1.0)
            } else {
                avg
            }
        };

        let output_key = node
            .config
            .get("output")
            .and_then(|o| o.as_str())
            .unwrap_or("aggregated_confidence");

        info!(
            node_id = %node.id,
            claims_count = claims_array.len(),
            aggregated_confidence = aggregated_confidence,
            "Bayesian aggregation complete"
        );

        Ok(NodeResult {
            node_id: node.id.clone(),
            success: true,
            output: serde_json::json!({
                output_key: aggregated_confidence,
                "sources_count": claims_array.len(),
                "deduplicated": deduplicate
            }),
            duration_ms: 0,
            error: None,
        })
    }

    /// Execute a KnowledgeDecay node - applies time-based confidence decay
    async fn execute_knowledge_decay_node(
        &self,
        ctx: ExecutionContext,
        node: &WorkflowNode,
    ) -> Result<NodeResult> {
        // Get input parameters
        let confidence = node
            .config
            .get("confidence")
            .and_then(|c| c.as_f64())
            .or_else(|| ctx.variables.get("confidence").and_then(|v| v.as_f64()))
            .ok_or_else(|| WorkflowError::ExecutionError("Missing confidence".to_string()))?;

        let lambda = node
            .config
            .get("lambda")
            .and_then(|l| l.as_f64())
            .unwrap_or(0.01); // Default: 1% per hour

        let time_delta_hours = node
            .config
            .get("time_delta_hours")
            .and_then(|t| t.as_f64())
            .or_else(|| {
                ctx.variables
                    .get("time_delta_hours")
                    .and_then(|v| v.as_f64())
            })
            .unwrap_or(24.0); // Default: 24 hours

        let activation_weight = node
            .config
            .get("activation_weight")
            .and_then(|a| a.as_f64())
            .unwrap_or(0.5);

        // Check if revalidation boost should be applied
        let revalidation_boost = node
            .config
            .get("revalidation_boost")
            .and_then(|r| r.as_f64());

        let new_confidence = if let Some(boost) = revalidation_boost {
            calculate_decay_with_revalidation(
                confidence,
                lambda,
                time_delta_hours,
                activation_weight,
                boost,
            )
        } else {
            calculate_decay(confidence, lambda, time_delta_hours, activation_weight)
        };

        // Check if revalidation is needed
        let threshold = node
            .config
            .get("revalidation_threshold")
            .and_then(|t| t.as_f64())
            .unwrap_or(0.3);

        let max_age = node
            .config
            .get("max_age_hours")
            .and_then(|m| m.as_f64())
            .unwrap_or(720.0); // 30 days

        let needs_rev = needs_revalidation(new_confidence, threshold, time_delta_hours, max_age);

        debug!(
            node_id = %node.id,
            original_confidence = confidence,
            new_confidence = new_confidence,
            needs_revalidation = needs_rev,
            "Knowledge decay applied"
        );

        let output_key = node
            .config
            .get("output")
            .and_then(|o| o.as_str())
            .unwrap_or("decayed_confidence");

        Ok(NodeResult {
            node_id: node.id.clone(),
            success: true,
            output: serde_json::json!({
                output_key: new_confidence,
                "original_confidence": confidence,
                "needs_revalidation": needs_rev,
                "decay_applied": new_confidence < confidence
            }),
            duration_ms: 0,
            error: None,
        })
    }

    /// Execute a ConflictDetection node - detects conflicts between claims
    async fn execute_conflict_detection_node(
        &self,
        ctx: ExecutionContext,
        node: &WorkflowNode,
    ) -> Result<NodeResult> {
        // Get new claim from input
        let new_claim_key = node
            .config
            .get("new_claim")
            .and_then(|c| c.as_str())
            .unwrap_or("new_claim");

        let new_claim_json = ctx
            .variables
            .get(new_claim_key)
            .or_else(|| ctx.inputs.get(new_claim_key))
            .cloned()
            .ok_or_else(|| {
                WorkflowError::ExecutionError(format!("New claim not found: {}", new_claim_key))
            })?;

        // Get existing claims to compare against
        let existing_claims_key = node
            .config
            .get("existing_claims")
            .and_then(|c| c.as_str())
            .unwrap_or("existing_claims");

        let existing_claims = ctx
            .variables
            .get(existing_claims_key)
            .or_else(|| ctx.inputs.get(existing_claims_key))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // For simplified workflow execution, we'll do content-based conflict detection
        // In production, this would use the full EpistemicClaim model
        let new_content = new_claim_json
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("");

        let mut conflicts = Vec::new();

        for existing in &existing_claims {
            let existing_content = existing
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("");

            // Simple conflict detection: check for negation patterns
            let has_negation = |s: &str| -> bool {
                let lower = s.to_lowercase();
                lower.contains("not ") || lower.contains("no ") || lower.contains("never")
            };

            if has_negation(new_content) != has_negation(existing_content) {
                // Check if they're about the same topic (simple word overlap)
                let new_words: std::collections::HashSet<_> =
                    new_content.split_whitespace().collect();
                let existing_words: std::collections::HashSet<_> =
                    existing_content.split_whitespace().collect();
                let overlap: std::collections::HashSet<_> =
                    new_words.intersection(&existing_words).collect();

                if !overlap.is_empty() && overlap.len() >= 3 {
                    conflicts.push(serde_json::json!({
                        "existing_claim_id": existing.get("id").cloned().unwrap_or(Value::Null),
                        "conflict_type": "DirectContradiction",
                        "similarity": overlap.len() as f64 / new_words.len().max(existing_words.len()) as f64
                    }));
                }
            }
        }

        let conflict_count = conflicts.len();
        let has_conflicts = conflict_count > 0;

        info!(
            node_id = %node.id,
            conflict_count = conflict_count,
            "Conflict detection complete"
        );

        let output_key = node
            .config
            .get("output")
            .and_then(|o| o.as_str())
            .unwrap_or("conflicts");

        Ok(NodeResult {
            node_id: node.id.clone(),
            success: true,
            output: serde_json::json!({
                output_key: conflicts,
                "has_conflicts": has_conflicts,
                "conflict_count": conflict_count
            }),
            duration_ms: 0,
            error: None,
        })
    }

    /// Execute a Revalidation node - applies boost to claim confidence
    async fn execute_revalidation_node(
        &self,
        ctx: ExecutionContext,
        node: &WorkflowNode,
    ) -> Result<NodeResult> {
        // Get current confidence
        let confidence = node
            .config
            .get("confidence")
            .and_then(|c| c.as_f64())
            .or_else(|| ctx.variables.get("confidence").and_then(|v| v.as_f64()))
            .ok_or_else(|| WorkflowError::ExecutionError("Missing confidence".to_string()))?;

        // Get revalidation boost
        let boost = node
            .config
            .get("boost")
            .and_then(|b| b.as_f64())
            .or_else(|| ctx.variables.get("boost").and_then(|v| v.as_f64()))
            .unwrap_or(0.2); // Default 20% boost

        let time_delta_hours = node
            .config
            .get("time_delta_hours")
            .and_then(|t| t.as_f64())
            .unwrap_or(0.0);

        let activation_weight = node
            .config
            .get("activation_weight")
            .and_then(|a| a.as_f64())
            .unwrap_or(0.5);

        // Calculate boosted confidence
        let new_confidence = calculate_decay_with_revalidation(
            confidence,
            0.01, // base lambda
            time_delta_hours,
            activation_weight,
            boost,
        );

        let actual_boost = new_confidence - confidence;

        info!(
            node_id = %node.id,
            original_confidence = confidence,
            new_confidence = new_confidence,
            boost_applied = actual_boost,
            "Revalidation applied"
        );

        let output_key = node
            .config
            .get("output")
            .and_then(|o| o.as_str())
            .unwrap_or("revvalidated_confidence");

        Ok(NodeResult {
            node_id: node.id.clone(),
            success: true,
            output: serde_json::json!({
                output_key: new_confidence,
                "original_confidence": confidence,
                "boost_applied": actual_boost,
                "boost_percentage": boost * 100.0
            }),
            duration_ms: 0,
            error: None,
        })
    }

    fn evaluate_condition(&self, condition: &Condition, state: &WorkflowState) -> bool {
        match condition {
            Condition::Always => true,
            Condition::Never => false,
            Condition::Expression(expr) => self.evaluate_simple_expression(expr, state),
        }
    }

    fn evaluate_simple_condition(&self, condition: &str, _ctx: &ExecutionContext) -> bool {
        // Simple condition evaluation - can be extended
        condition.contains("true") || condition.contains("success")
    }

    fn evaluate_simple_expression(&self, expr: &str, _state: &WorkflowState) -> bool {
        // Simple expression evaluation
        expr.contains("true") || expr.contains("success")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_state_creation() {
        let state = WorkflowState::new("test-workflow".to_string());
        assert_eq!(state.workflow_id, "test-workflow");
        assert_eq!(state.status, ExecutionStatus::Pending);
        assert!(state.completed_nodes.is_empty());
        assert!(state.failed_nodes.is_empty());
    }

    #[test]
    fn test_workflow_state_variables() {
        let mut state = WorkflowState::new("test".to_string());
        state.set_variable("key".to_string(), serde_json::json!("value"));
        assert_eq!(state.get_variable("key"), Some(&serde_json::json!("value")));
    }

    #[test]
    fn test_workflow_state_is_complete() {
        let mut state = WorkflowState::new("test".to_string());
        state.completed_nodes.insert("node1".to_string());
        state.completed_nodes.insert("node2".to_string());
        assert!(state.is_complete(2));
        assert!(!state.is_complete(3));
    }

    #[test]
    fn test_execution_context() {
        let ctx = ExecutionContext {
            workflow_id: "wf1".to_string(),
            node_id: "node1".to_string(),
            variables: HashMap::new(),
            inputs: HashMap::from([("input1".to_string(), serde_json::json!("test"))]),
        };
        assert_eq!(ctx.get_input("input1"), Some(&serde_json::json!("test")));
    }

    #[tokio::test]
    async fn test_workflow_engine_creation() {
        let engine = WorkflowEngine::new();
        assert!(engine.workflows.is_empty());
    }

    #[tokio::test]
    async fn test_start_execution_not_found() {
        let engine = WorkflowEngine::new();
        let result = engine.start_execution("nonexistent", HashMap::new());
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_workflow_engine_register_and_execute() {
        use crate::engine::{EdgeType, WorkflowNode};

        let engine = WorkflowEngine::new();
        let mut graph = DiGraph::new();
        let start = graph.add_node(WorkflowNode {
            id: "start".to_string(),
            node_type: NodeType::Task,
            config: serde_json::json!({}),
            handler: None,
        });
        let end = graph.add_node(WorkflowNode {
            id: "end".to_string(),
            node_type: NodeType::Task,
            config: serde_json::json!({}),
            handler: None,
        });
        graph.add_edge(start, end, EdgeType::Sequential);

        let workflow = Workflow {
            id: "test-workflow".to_string(),
            name: "Test".to_string(),
            graph,
            start_node: start,
        };
        engine.register_workflow(workflow);

        let execution_id = engine
            .start_execution("test-workflow", HashMap::new())
            .unwrap();
        assert!(!execution_id.is_empty());

        // Wait a bit for async processing
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let state = engine.get_execution_state(&execution_id).await;
        assert!(state.is_some());
    }

    #[test]
    fn test_node_type_variants() {
        assert_eq!(NodeType::Task, NodeType::Task);
        assert_eq!(NodeType::Condition, NodeType::Condition);
        assert_eq!(NodeType::Parallel, NodeType::Parallel);
    }

    #[test]
    fn test_edge_type_variants() {
        assert_eq!(EdgeType::Sequential, EdgeType::Sequential);
        assert_eq!(
            EdgeType::Conditional {
                condition: Condition::Always
            },
            EdgeType::Conditional {
                condition: Condition::Always
            }
        );
    }

    #[test]
    fn test_condition_variants() {
        assert_eq!(Condition::Always, Condition::Always);
        assert_eq!(Condition::Never, Condition::Never);
        assert_eq!(
            Condition::Expression("test".to_string()),
            Condition::Expression("test".to_string())
        );
    }
}
