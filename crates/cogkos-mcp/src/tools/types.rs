//! Request/response types for MCP tools

use cogkos_core::models::*;
use serde::{Deserialize, Serialize};

/// Meta directory entry representing a domain in the knowledge directory
#[derive(Debug, Clone, Serialize)]
pub struct MetaDirectoryEntry {
    pub domain: String,
    pub claim_count: usize,
    pub expertise_score: f64,
    pub node_types: std::collections::HashMap<String, usize>,
    pub avg_confidence: f64,
    pub latest_update: Option<chrono::DateTime<chrono::Utc>>,
}

/// Query knowledge request
#[derive(Debug, Deserialize)]
pub struct QueryKnowledgeRequest {
    pub query: String,
    #[serde(default)]
    pub context: QueryContext,
    #[serde(default)]
    pub knowledge_types: Option<Vec<String>>,
    #[serde(default)]
    pub entity_refs: Option<Vec<serde_json::Value>>,
    #[serde(default = "default_true")]
    pub include_predictions: bool,
    #[serde(default = "default_true")]
    pub include_conflicts: bool,
    #[serde(default = "default_true")]
    pub include_gaps: bool,
    /// Minimum activation threshold for graph diffusion (0.0-1.0)
    #[serde(default = "default_activation_threshold")]
    pub activation_threshold: f64,
    /// Delegate to sampling protocol for advanced analysis
    #[serde(default)]
    pub delegate_to_sampling: bool,
    /// Filter results by memory layer: "working", "episodic", "semantic", or None (all)
    #[serde(default)]
    pub memory_layer: Option<String>,
    /// Filter results by session ID (for working/episodic memory)
    #[serde(default)]
    pub session_id: Option<String>,
    /// Agent ID for episodic memory scoping — episodic results filtered to this agent only
    #[serde(default)]
    pub agent_id: Option<String>,
}

pub fn default_activation_threshold() -> f64 {
    0.3
}

#[derive(Debug, Deserialize)]
pub struct QueryContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default)]
    pub urgency: Urgency,
    #[serde(default = "default_max_results")]
    pub max_results: u32,
}

impl Default for QueryContext {
    fn default() -> Self {
        Self {
            domain: None,
            urgency: Urgency::Normal,
            max_results: 10,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Urgency {
    #[default]
    Normal,
    Low,
    High,
}

pub fn default_true() -> bool {
    true
}
pub fn default_max_results() -> u32 {
    10
}

/// Submit experience request
#[derive(Debug, Deserialize)]
pub struct SubmitExperienceRequest {
    pub content: String,
    pub node_type: NodeType,
    #[serde(default)]
    pub knowledge_type: Option<String>,
    #[serde(default)]
    pub structured_content: Option<serde_json::Value>,
    #[serde(default)]
    pub entity_refs: Vec<serde_json::Value>,
    #[serde(default)]
    pub confidence: Option<f64>,
    pub source: SourceInfo,
    #[serde(default)]
    pub valid_from: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub valid_to: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub related_to: Vec<uuid::Uuid>,
    /// Memory layer: "working", "episodic", or "semantic" (default)
    #[serde(default)]
    pub memory_layer: Option<String>,
    /// Session ID for working/episodic memory scoping
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SourceInfo {
    Human { user_id: String },
    Agent { agent_id: String, model: String },
    External { source_name: String },
}

/// Submit feedback request
#[derive(Debug, Deserialize)]
pub struct SubmitFeedbackRequest {
    pub query_hash: u64,
    pub success: bool,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub improvement_suggestion: Option<String>,
    /// Agent identity for feedback attribution.
    /// Falls back to `{tenant_id}/anonymous` when absent.
    #[serde(default)]
    pub agent_id: Option<String>,
}

/// Report gap request
#[derive(Debug, Deserialize)]
pub struct ReportGapRequest {
    pub domain: String,
    pub description: String,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub suggested_sources: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    #[default]
    Medium,
    Low,
    High,
}

/// Get meta directory request
#[derive(Debug, Deserialize, Default)]
pub struct GetMetaDirectoryRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_domain: Option<String>,
    #[serde(default)]
    pub min_expertise_score: Option<f64>,
}

/// Cross-instance query request
#[derive(Debug, Deserialize, Default)]
pub struct CrossInstanceQueryRequest {
    pub query: String,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub priority: Urgency,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_min_results")]
    pub min_results: usize,
}

pub fn default_timeout_ms() -> u64 {
    30000
}

pub fn default_min_results() -> usize {
    1
}

// === Subscription Management Requests (Issue #132) ===

/// Subscribe to RSS feed request
#[derive(Debug, Deserialize)]
pub struct SubscribeRssRequest {
    pub url: String,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_max_items")]
    pub max_items: usize,
    #[serde(default)]
    pub fetch_full_content: bool,
}

pub fn default_poll_interval() -> u64 {
    3600
}

pub fn default_max_items() -> usize {
    20
}

/// Register webhook subscription request
#[derive(Debug, Deserialize)]
pub struct SubscribeWebhookRequest {
    pub url: String,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub events: Vec<String>,
}

/// API polling subscription request
#[derive(Debug, Deserialize)]
pub struct SubscribeApiRequest {
    pub url: String,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_http_method")]
    pub method: String,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
}

pub fn default_http_method() -> String {
    "GET".to_string()
}

/// Subscription response
#[derive(Debug, Serialize)]
pub struct SubscriptionResponse {
    pub subscription_id: String,
    pub status: String,
    pub message: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// List subscriptions request
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ListSubscriptionsRequest {
    Rss,
    Webhook,
    Api,
}

/// Cross-instance query response
#[derive(Debug, Serialize)]
pub struct CrossInstanceQueryResponse {
    pub query_id: String,
    pub results: Vec<CrossInstanceResult>,
    pub aggregated: Option<AggregatedInsight>,
    pub metadata: CrossInstanceMetadata,
}

#[derive(Debug, Serialize)]
pub struct CrossInstanceResult {
    pub node_id: String,
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
    pub response_time_ms: u64,
    pub expertise_score: f64,
}

#[derive(Debug, Serialize)]
pub struct AggregatedInsight {
    pub content: String,
    pub confidence: f64,
    pub sources: Vec<String>,
    pub coverage_score: f64,
}

#[derive(Debug, Serialize)]
pub struct CrossInstanceMetadata {
    pub total_nodes: usize,
    pub successful_nodes: usize,
    pub failed_nodes: usize,
    pub processing_time_ms: u64,
}

/// Upload document request
#[derive(Debug, Deserialize)]
pub struct UploadDocumentRequest {
    pub filename: String,
    #[serde(rename = "content_base64")]
    pub content: String,
    pub source: SourceInfo,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_true")]
    pub auto_process: bool,
}

/// Document upload response
#[derive(Debug, Serialize)]
pub struct DocumentUploadResponse {
    pub file_id: String,
    pub status: String,
    pub estimated_time: String,
    pub pipeline_id: Option<String>,
    #[serde(default)]
    pub is_duplicate: bool,
}

/// Knowledge gap record (for report_gap)
#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeGap {
    pub gap_id: uuid::Uuid,
    pub domain: String,
    pub description: String,
    pub priority: String,
    pub suggested_sources: Vec<String>,
    pub status: String,
    pub reported_at: chrono::DateTime<chrono::Utc>,
    pub estimated_fill_time: Option<String>,
}
