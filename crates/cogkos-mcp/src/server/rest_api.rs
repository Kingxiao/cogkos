//! Native REST API — bypasses MCP protocol layer (SSE + JSON-RPC) for lower latency.
//!
//! Reuses all business logic from `crate::tools::*` handlers. Only the protocol
//! envelope is different: plain JSON POST/response instead of MCP framing.

use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::McpServerState;
use crate::tools::*;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
}

pub type ErrResponse = (StatusCode, Json<ErrorBody>);

fn err(status: StatusCode, msg: impl Into<String>) -> ErrResponse {
    (status, Json(ErrorBody { error: msg.into() }))
}

type RestResult<T> = Result<Json<T>, ErrResponse>;

// ---------------------------------------------------------------------------
// Auth helper
// ---------------------------------------------------------------------------

fn extract_api_key(headers: &HeaderMap, body_key: Option<&str>) -> Result<String, ErrResponse> {
    if let Some(hdr) = headers.get("x-api-key") {
        return hdr
            .to_str()
            .map(|s| s.to_string())
            .map_err(|_| err(StatusCode::BAD_REQUEST, "Invalid X-API-Key header"));
    }
    if let Some(key) = body_key {
        return Ok(key.to_string());
    }
    if let Ok(key) = std::env::var("DEFAULT_MCP_API_KEY") {
        if !key.is_empty() {
            return Ok(key);
        }
    }
    Err(err(
        StatusCode::UNAUTHORIZED,
        "Missing API key: provide X-API-Key header or api_key field",
    ))
}

/// Authenticate and rate-limit. Returns tenant auth context.
async fn authenticate(
    state: &McpServerState,
    headers: &HeaderMap,
    body_key: Option<&str>,
) -> Result<crate::AuthContext, ErrResponse> {
    let api_key = extract_api_key(headers, body_key)?;

    let auth = state
        .auth
        .authenticate(&api_key)
        .await
        .map_err(|e| err(StatusCode::UNAUTHORIZED, e.to_string()))?;

    state
        .rate_limiter
        .check(&auth.tenant_id)
        .await
        .map_err(|e| err(StatusCode::TOO_MANY_REQUESTS, e.message))?;

    Ok(auth)
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct RestQueryRequest {
    pub query: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub max_results: Option<u32>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub memory_layer: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default = "default_true")]
    pub include_conflicts: bool,
    #[serde(default)]
    pub include_predictions: bool,
    #[serde(default)]
    pub include_gaps: bool,
}

fn default_true() -> bool {
    true
}

pub async fn rest_query_handler(
    State(state): State<McpServerState>,
    headers: HeaderMap,
    Json(req): Json<RestQueryRequest>,
) -> RestResult<serde_json::Value> {
    let start = std::time::Instant::now();

    let auth = authenticate(&state, &headers, req.api_key.as_deref()).await?;
    if !auth.can_read() {
        return Err(err(StatusCode::FORBIDDEN, "Read permission required"));
    }

    let query_req = QueryKnowledgeRequest {
        query: req.query,
        context: QueryContext {
            domain: req.domain,
            max_results: req.max_results.unwrap_or(10),
            ..Default::default()
        },
        namespace: req.namespace,
        memory_layer: req.memory_layer,
        session_id: req.session_id,
        include_conflicts: req.include_conflicts,
        include_predictions: req.include_predictions,
        include_gaps: req.include_gaps,
        ..default_query_request()
    };

    let response = handle_query_knowledge(
        query_req,
        &auth.tenant_id,
        &[],
        state.stores.claims.as_ref(),
        state.stores.vectors.as_ref(),
        state.stores.graph.as_ref(),
        state.stores.cache.as_ref(),
        state.stores.gaps.as_ref(),
        state.llm_client.clone(),
        state.embedding_client.clone(),
        Some(&state.activation_buffer),
    )
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let elapsed = start.elapsed().as_millis() as u64;
    info!(elapsed_ms = elapsed, "REST query completed");
    cogkos_core::monitoring::METRICS.inc_counter("cogkos_rest_query_total", 1);

    Ok(Json(serde_json::to_value(response).unwrap_or_default()))
}

/// Build a default QueryKnowledgeRequest (serde defaults for fields not exposed in REST).
fn default_query_request() -> QueryKnowledgeRequest {
    serde_json::from_value(serde_json::json!({"query": ""})).unwrap()
}

// ---------------------------------------------------------------------------
// Learn (submit_experience)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct RestLearnRequest {
    pub content: String,
    pub node_type: cogkos_core::models::NodeType,
    pub source: SourceInfo,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub knowledge_type: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub memory_layer: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub session_date: Option<String>,
}

pub async fn rest_learn_handler(
    State(state): State<McpServerState>,
    headers: HeaderMap,
    Json(req): Json<RestLearnRequest>,
) -> RestResult<serde_json::Value> {
    let auth = authenticate(&state, &headers, req.api_key.as_deref()).await?;
    if !auth.can_write() {
        return Err(err(StatusCode::FORBIDDEN, "Write permission required"));
    }

    let exp_req = SubmitExperienceRequest {
        content: req.content,
        node_type: req.node_type,
        source: req.source,
        confidence: req.confidence,
        knowledge_type: req.knowledge_type,
        tags: req.tags,
        namespace: req.namespace,
        memory_layer: req.memory_layer,
        session_id: req.session_id,
        session_date: req.session_date,
        structured_content: None,
        entity_refs: vec![],
        valid_from: None,
        valid_to: None,
        related_to: vec![],
    };

    let result = handle_submit_experience(
        exp_req,
        &auth.tenant_id,
        Arc::clone(&state.stores.claims),
        Arc::clone(&state.stores.vectors),
        Arc::clone(&state.stores.graph),
        state.embedding_client.clone(),
    )
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    cogkos_core::monitoring::METRICS.inc_counter("cogkos_rest_learn_total", 1);
    Ok(Json(result))
}

// ---------------------------------------------------------------------------
// Feedback
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct RestFeedbackRequest {
    pub query_hash: u64,
    pub success: bool,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub improvement_suggestion: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
}

pub async fn rest_feedback_handler(
    State(state): State<McpServerState>,
    headers: HeaderMap,
    Json(req): Json<RestFeedbackRequest>,
) -> RestResult<serde_json::Value> {
    let auth = authenticate(&state, &headers, req.api_key.as_deref()).await?;
    if !auth.can_write() {
        return Err(err(StatusCode::FORBIDDEN, "Write permission required"));
    }

    let agent_id = req
        .agent_id
        .clone()
        .unwrap_or_else(|| format!("{}/anonymous", auth.tenant_id));

    let fb_req = SubmitFeedbackRequest {
        query_hash: req.query_hash,
        success: req.success,
        note: req.note,
        improvement_suggestion: req.improvement_suggestion,
        agent_id: Some(agent_id.clone()),
    };

    let result = handle_submit_feedback(
        fb_req,
        &auth.tenant_id,
        &agent_id,
        state.stores.feedback.as_ref(),
        state.stores.cache.as_ref(),
        state.stores.claims.as_ref(),
    )
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    cogkos_core::monitoring::METRICS.inc_counter("cogkos_rest_feedback_total", 1);
    Ok(Json(result))
}
