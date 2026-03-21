//! # CogKOS Federation Layer
//!
//! Partially active. Single-instance modules (collective wisdom, aggregation, health)
//! are used for multi-agent knowledge quality monitoring.
//! Cross-instance modules (routing, cross_instance, protocol, node) remain frozen for V2/V3.

pub mod aggregation;
pub mod collective_wisdom;
pub mod error;
pub mod federation_impl;
pub mod health;
pub mod node;
pub mod routing;

pub use aggregation::{
    AggregatedResponse, AggregationConfig, AggregationMethod, FederatedResult, NodeResult,
    ResultAggregator, SmartAggregator, WeightedAggregator,
};
pub use collective_wisdom::{
    CollectiveWisdomHealth, CollectiveWisdomHealthChecker, CollectiveWisdomMetrics,
    HealthCheckConfig, NodeResponse,
};
pub use error::{FederationError, Result};
pub use federation_impl::{
    AnonymizationConfig, AnonymousInsight, CrossInstanceAuth, CrossInstanceAuthenticator,
    FederationExport, FederationManager, FederationPermission, FederationProtocol,
    FederationProtocolError, HttpFederationProtocol, InsightAnonymizer, InsightStatistics,
    TimeBucket, ValidationMetadata, ValidationResult,
};
pub use health::{
    CollectiveIntelligenceHealth, ConditionResults, DiversityResult, HealthStatus,
    IndependenceResult, InsightSource, Prediction, ProvenanceInfo, calculate_collective_health,
};
pub use node::{FederatedNode, NodeHealth, NodeRegistry, NodeStatus};
pub use routing::{
    AdaptiveRouter, DomainRouter, FederatedQuery, MetadataDirectoryRouter, NodeRoute,
    QueryPriority, QueryRouter, RoutingDecision, RoutingStrategy,
};

use aggregation::AggregationMetadata;
use futures::future::join_all;
use reqwest::Client;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

/// Main federation client for executing federated queries
pub struct FederationClient {
    registry: Arc<tokio::sync::RwLock<NodeRegistry>>,
    router: Arc<dyn QueryRouter>,
    http_client: Client,
    default_config: AggregationConfig,
}

impl FederationClient {
    pub fn new(registry: NodeRegistry, router: Arc<dyn QueryRouter>) -> Result<Self> {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(FederationError::NetworkError)?;

        Ok(Self {
            registry: Arc::new(tokio::sync::RwLock::new(registry)),
            router,
            http_client,
            default_config: AggregationConfig::default(),
        })
    }

    pub fn with_config(mut self, config: AggregationConfig) -> Self {
        self.default_config = config;
        self
    }

    /// Execute a federated query across multiple nodes
    pub async fn query(&self, query: FederatedQuery) -> Result<FederatedResult> {
        let start = Instant::now();
        let query_id = query.query_id.clone();

        debug!(
            "Executing federated query {} to domains {:?}",
            query_id, query.domains
        );

        // Get routing decision
        let registry = self.registry.read().await;
        let routing = self.router.route(&query, &registry).await?;
        drop(registry);

        let target_count = routing.target_nodes.len();
        info!("Query {} routed to {} nodes", query_id, target_count);

        // Execute queries to all target nodes in parallel
        let mut futures = Vec::new();
        for route in &routing.target_nodes {
            let future =
                self.execute_node_query(route.clone(), query.query_text.clone(), query.timeout_ms);
            futures.push(future);
        }

        // Wait for all queries to complete (with timeout)
        let results = join_all(futures).await;

        let node_results: Vec<NodeResult> = results.into_iter().collect();
        let successful_count = node_results.iter().filter(|r| r.success).count();
        let failed_count = node_results.len() - successful_count;

        info!(
            "Query {} completed: {}/{} successful",
            query_id, successful_count, target_count
        );

        // Check minimum success rate
        let success_rate = successful_count as f64 / target_count as f64;
        if success_rate < self.default_config.min_success_rate {
            warn!(
                "Query {} success rate {:.2} below threshold {:.2}",
                query_id, success_rate, self.default_config.min_success_rate
            );
        }

        // Aggregate results
        let aggregator = self.default_config.create_aggregator();
        let aggregated = match aggregator.aggregate(node_results.clone()) {
            Ok(result) => Some(result),
            Err(e) => {
                error!("Aggregation failed for query {}: {}", query_id, e);
                None
            }
        };

        let processing_time = start.elapsed().as_millis() as u64;

        // Extract metadata before moving aggregated
        let consensus_reached = aggregated
            .as_ref()
            .map(|a| a.confidence > 0.7)
            .unwrap_or(false);
        let consensus_score = aggregated.as_ref().map(|a| a.confidence);

        Ok(FederatedResult {
            query_id: query_id.clone(),
            node_results,
            aggregated,
            metadata: AggregationMetadata {
                total_nodes: target_count,
                successful_nodes: successful_count,
                failed_nodes: failed_count,
                consensus_reached,
                consensus_score,
                aggregation_method: format!("{:?}", self.default_config.method),
                processing_time_ms: processing_time,
            },
        })
    }

    async fn execute_node_query(
        &self,
        route: NodeRoute,
        query_text: String,
        timeout_ms: u64,
    ) -> NodeResult {
        let start = Instant::now();
        let node_id = route.node_id.clone();

        // Prepare request payload
        let payload = serde_json::json!({
            "query": query_text,
            "node_id": node_id,
        });

        // Execute with timeout
        let result = timeout(
            Duration::from_millis(timeout_ms),
            self.http_client.post(&route.endpoint).json(&payload).send(),
        )
        .await;

        let response_time = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(response)) => {
                if response.status().is_success() {
                    match response.json::<serde_json::Value>().await {
                        Ok(data) => NodeResult {
                            node_id,
                            success: true,
                            data: Some(data),
                            error: None,
                            response_time_ms: response_time,
                            expertise_score: route.expected_expertise,
                        },
                        Err(e) => NodeResult {
                            node_id,
                            success: false,
                            data: None,
                            error: Some(format!("Parse error: {}", e)),
                            response_time_ms: response_time,
                            expertise_score: route.expected_expertise,
                        },
                    }
                } else {
                    let status = response.status();
                    let error_text = response.text().await.unwrap_or_default();
                    NodeResult {
                        node_id,
                        success: false,
                        data: None,
                        error: Some(format!("HTTP {}: {}", status, error_text)),
                        response_time_ms: response_time,
                        expertise_score: route.expected_expertise,
                    }
                }
            }
            Ok(Err(e)) => {
                error!("Request to node {} failed: {}", node_id, e);
                NodeResult {
                    node_id,
                    success: false,
                    data: None,
                    error: Some(format!("Request failed: {}", e)),
                    response_time_ms: response_time,
                    expertise_score: route.expected_expertise,
                }
            }
            Err(_) => {
                warn!("Query to node {} timed out after {}ms", node_id, timeout_ms);
                NodeResult {
                    node_id,
                    success: false,
                    data: None,
                    error: Some("Timeout".to_string()),
                    response_time_ms: timeout_ms,
                    expertise_score: route.expected_expertise,
                }
            }
        }
    }

    /// Register a new federated node
    pub async fn register_node(&self, node: FederatedNode) {
        let mut registry = self.registry.write().await;
        registry.register(node);
        info!(
            "Registered federated node, total nodes: {}",
            registry.list().len()
        );
    }

    /// Unregister a node
    pub async fn unregister_node(&self, node_id: &str) -> Option<FederatedNode> {
        let mut registry = self.registry.write().await;
        let removed = registry.unregister(node_id);
        if removed.is_some() {
            info!("Unregistered federated node {}", node_id);
        }
        removed
    }

    /// Get all registered nodes
    pub async fn list_nodes(&self) -> Vec<FederatedNode> {
        let registry = self.registry.read().await;
        registry.list().into_iter().cloned().collect()
    }

    /// Update node health status
    pub async fn update_node_health(&self, node_id: &str, health: NodeHealth) {
        let mut registry = self.registry.write().await;
        if let Some(node) = registry.get_mut(node_id) {
            node.health_status = health;
            node.update_heartbeat();
        }
    }

    /// Health check all nodes
    pub async fn health_check(&self) -> Vec<(String, NodeHealth)> {
        let nodes = self.list_nodes().await;
        let mut results = Vec::new();

        for node in nodes {
            // Simple health check - ping the node
            let health = match self
                .http_client
                .get(format!("{}/health", node.endpoint))
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => NodeHealth::Healthy,
                Ok(resp) => NodeHealth::Degraded {
                    reason: format!("HTTP {}", resp.status()),
                },
                Err(e) => NodeHealth::Unhealthy {
                    reason: e.to_string(),
                },
            };

            self.update_node_health(&node.id, health.clone()).await;
            results.push((node.id, health));
        }

        results
    }
}

/// Builder for creating federation clients
pub struct FederationClientBuilder {
    router: Option<Arc<dyn QueryRouter>>,
    config: Option<AggregationConfig>,
}

impl FederationClientBuilder {
    pub fn new() -> Self {
        Self {
            router: None,
            config: None,
        }
    }

    pub fn with_router(mut self, router: Arc<dyn QueryRouter>) -> Self {
        self.router = Some(router);
        self
    }

    pub fn with_config(mut self, config: AggregationConfig) -> Self {
        self.config = Some(config);
        self
    }

    pub fn build(self) -> Result<FederationClient> {
        let registry = NodeRegistry::new();
        let router = self
            .router
            .unwrap_or_else(|| Arc::new(routing::DomainRouter::new()));

        let client = FederationClient::new(registry, router)?;

        if let Some(config) = self.config {
            Ok(client.with_config(config))
        } else {
            Ok(client)
        }
    }
}

impl Default for FederationClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_registry() {
        let mut registry = NodeRegistry::new();

        let node = FederatedNode::new("test-1", "Test Node", "http://localhost:8080")
            .with_domains(vec!["domain1".to_string(), "domain2".to_string()])
            .with_expertise("domain1", 0.9);

        registry.register(node);

        assert_eq!(registry.list().len(), 1);
        assert_eq!(registry.find_by_domain("domain1").len(), 1);

        let retrieved = registry.get("test-1").unwrap();
        assert_eq!(retrieved.expertise_for("domain1"), 0.9);
    }

    #[test]
    fn test_aggregation_config() {
        let config = AggregationConfig {
            method: AggregationMethod::Weighted,
            ..Default::default()
        };

        let _aggregator = config.create_aggregator();
        // Just verify it creates without panicking
    }
}
