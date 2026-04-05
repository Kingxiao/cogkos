//! Federation MCP tools — cross-instance queries, node management, health monitoring

use std::sync::Arc;

use cogkos_federation::{
    AnonymizationConfig, FederatedNode, FederatedQuery, FederationClient, FederationError,
    InsightAnonymizer, QueryPriority, TimeBucket,
};
use cogkos_store::ClaimStore;
use serde::{Deserialize, Serialize};

/// Register a remote CogKOS instance as a federation node
#[derive(Debug, Deserialize)]
pub struct RegisterNodeRequest {
    pub node_id: String,
    pub name: String,
    pub endpoint: String,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub expertise: std::collections::HashMap<String, f64>,
}

/// Federated query across multiple CogKOS instances
#[derive(Debug, Deserialize)]
pub struct FederationQueryRequest {
    pub query: String,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub priority: String,
}

fn default_timeout() -> u64 {
    30000
}

/// Export anonymized insights
#[derive(Debug, Deserialize)]
pub struct ExportInsightsRequest {
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default = "default_confidence")]
    pub min_confidence: f64,
}

fn default_confidence() -> f64 {
    0.5
}

/// Federation health response
#[derive(Debug, Serialize)]
pub struct FederationHealthResponse {
    pub enabled: bool,
    pub total_nodes: usize,
    pub nodes: Vec<NodeHealthInfo>,
}

#[derive(Debug, Serialize)]
pub struct NodeHealthInfo {
    pub node_id: String,
    pub name: String,
    pub endpoint: String,
    pub health: String,
    pub domains: Vec<String>,
}

pub async fn handle_federation_register_node(
    req: RegisterNodeRequest,
    federation_client: Option<Arc<FederationClient>>,
) -> Result<serde_json::Value, FederationError> {
    let client = federation_client.ok_or(FederationError::NotEnabled)?;

    let mut node = FederatedNode::new(&req.node_id, &req.name, &req.endpoint)
        .with_domains(req.domains);

    for (domain, score) in &req.expertise {
        node = node.with_expertise(domain, *score);
    }

    client.register_node(node).await;

    Ok(serde_json::json!({
        "status": "registered",
        "node_id": req.node_id,
    }))
}

pub async fn handle_federation_query(
    req: FederationQueryRequest,
    federation_client: Option<Arc<FederationClient>>,
) -> Result<serde_json::Value, FederationError> {
    let client = federation_client.ok_or(FederationError::NotEnabled)?;

    let priority = match req.priority.as_str() {
        "high" => QueryPriority::High,
        "critical" => QueryPriority::Critical,
        "low" => QueryPriority::Low,
        _ => QueryPriority::Normal,
    };

    let query = FederatedQuery {
        query_id: uuid::Uuid::new_v4().to_string(),
        query_text: req.query,
        domains: req.domains,
        context: std::collections::HashMap::new(),
        priority,
        timeout_ms: req.timeout_ms,
        min_results: 1,
    };

    let result = client.query(query).await?;

    Ok(serde_json::json!({
        "query_id": result.query_id,
        "aggregated": result.aggregated.as_ref().map(|a| serde_json::json!({
            "content": a.content,
            "confidence": a.confidence,
            "sources": a.sources,
            "coverage_score": a.coverage_score,
        })),
        "node_results": result.node_results.len(),
        "metadata": {
            "total_nodes": result.metadata.total_nodes,
            "successful_nodes": result.metadata.successful_nodes,
            "processing_time_ms": result.metadata.processing_time_ms,
            "consensus_reached": result.metadata.consensus_reached,
        },
    }))
}

pub async fn handle_federation_health(
    federation_client: Option<Arc<FederationClient>>,
) -> Result<serde_json::Value, FederationError> {
    let client = match federation_client {
        Some(c) => c,
        None => {
            return Ok(serde_json::json!({
                "enabled": false,
                "total_nodes": 0,
                "nodes": [],
            }));
        }
    };

    let health_results = client.health_check().await;
    let nodes = client.list_nodes().await;

    let node_infos: Vec<serde_json::Value> = nodes
        .iter()
        .map(|n| {
            let health = health_results
                .iter()
                .find(|(id, _)| id == &n.id)
                .map(|(_, h)| format!("{:?}", h))
                .unwrap_or_else(|| "Unknown".to_string());

            serde_json::json!({
                "node_id": n.id,
                "name": n.name,
                "endpoint": n.endpoint,
                "health": health,
                "domains": n.domains,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "enabled": true,
        "total_nodes": nodes.len(),
        "nodes": node_infos,
    }))
}

pub async fn handle_federation_export_insights(
    req: ExportInsightsRequest,
    tenant_id: &str,
    claim_store: &dyn ClaimStore,
) -> Result<serde_json::Value, FederationError> {
    let instance_id = std::env::var("FEDERATION_INSTANCE_ID")
        .unwrap_or_else(|_| format!("cogkos-{}", &tenant_id[..8.min(tenant_id.len())]));

    let config = AnonymizationConfig {
        min_confidence: req.min_confidence,
        time_bucket: TimeBucket::Day,
        ..AnonymizationConfig::default()
    };

    let anonymizer = InsightAnonymizer::new(config);

    // Fetch claims — search by domain or get recent by stage
    let search_query = req.domain.as_deref().unwrap_or("*");
    let claims = claim_store
        .search_claims(tenant_id, search_query, 100)
        .await
        .map_err(|e| FederationError::CrossInstanceError(format!("Failed to fetch claims: {}", e)))?;

    // Filter by confidence threshold
    let filtered: Vec<_> = claims
        .into_iter()
        .filter(|c| c.confidence >= req.min_confidence)
        .collect();

    // Anonymize
    let insights: Vec<_> = filtered
        .iter()
        .filter_map(|claim| anonymizer.anonymize(claim, &instance_id).ok())
        .collect();

    // Hash the instance ID — never expose raw identity in federation exports
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    instance_id.hash(&mut hasher);
    let instance_hash = format!("{:016x}", hasher.finish());

    Ok(serde_json::json!({
        "exported": insights.len(),
        "instance_id_hash": instance_hash,
        "insights": insights,
    }))
}
