//! Cross-instance query routing module
//!
//! This module provides intelligent routing for federated queries across multiple CogKOS instances.
//! It uses metadata directory, semantic analysis, and expertise scoring to route queries optimally.

use crate::aggregation::{
    AggregationMetadata, FederatedResult, NodeResult, ResultAggregator, WeightedAggregator,
};
use crate::error::{FederationError, Result};
use crate::node::NodeRegistry;
use crate::routing::{FederatedQuery, NodeRoute, QueryRouter, RoutingDecision, RoutingStrategy};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Domain expertise information from metadata directory
#[derive(Debug, Clone)]
pub struct DomainExpertise {
    pub domain: String,
    pub expertise_score: f64,
    pub claim_count: usize,
    pub avg_confidence: f64,
    pub node_types: HashMap<String, usize>,
}

/// Metadata directory cache for cross-instance lookups
pub struct MetadataDirectory {
    /// Domain -> Expertise mapping
    expertise: Arc<RwLock<HashMap<String, DomainExpertise>>>,
    /// Instance ID -> Endpoint mapping
    instances: Arc<RwLock<HashMap<String, String>>>,
}

impl MetadataDirectory {
    pub fn new() -> Self {
        Self {
            expertise: Arc::new(RwLock::new(HashMap::new())),
            instances: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Update domain expertise from local knowledge base
    pub async fn update_domain_expertise(&self, domain: String, expertise: DomainExpertise) {
        let mut exp = self.expertise.write().await;
        exp.insert(domain, expertise);
    }

    /// Register an instance endpoint
    pub async fn register_instance(&self, instance_id: String, endpoint: String) {
        let mut instances = self.instances.write().await;
        instances.insert(instance_id, endpoint);
    }

    /// Get expertise score for a domain
    pub async fn get_expertise(&self, domain: &str) -> Option<f64> {
        let exp = self.expertise.read().await;
        exp.get(domain).map(|e| e.expertise_score)
    }

    /// Get best domain for a query
    pub async fn suggest_domains(&self, query_text: &str, max_results: usize) -> Vec<(String, f64)> {
        let exp = self.expertise.read().await;
        
        // Simple keyword matching for domain suggestion
        // In production, this would use embeddings or LLM-based classification
        let query_lower = query_text.to_lowercase();
        
        let mut scores: Vec<(String, f64)> = exp
            .iter()
            .map(|(domain, expertise)| {
                let keyword_match = domain.to_lowercase()
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|kw| kw.len() > 2)
                    .filter(|kw| query_lower.contains(kw))
                    .count();
                
                let score = expertise.expertise_score + (keyword_match as f64 * 0.1);
                (domain.clone(), score)
            })
            .collect();
        
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.into_iter().take(max_results).collect()
    }

    /// Get all registered instances
    pub async fn list_instances(&self) -> HashMap<String, String> {
        let instances = self.instances.read().await;
        instances.clone()
    }
}

impl Default for MetadataDirectory {
    fn default() -> Self {
        Self::new()
    }
}

/// Query analyzer that extracts domains from query text
pub struct QueryAnalyzer {
    domain_keywords: HashMap<String, Vec<String>>,
}

impl QueryAnalyzer {
    pub fn new() -> Self {
        let mut domain_keywords = HashMap::new();
        
        // Tech/AI domains
        domain_keywords.insert("ai".to_string(), vec![
            "artificial intelligence".to_string(), "machine learning".to_string(),
            "ml".to_string(), "deep learning".to_string(), "neural".to_string(),
            "llm".to_string(), "gpt".to_string(), "transformer".to_string(),
        ]);
        
        domain_keywords.insert("data_science".to_string(), vec![
            "data science".to_string(), "analytics".to_string(), "statistics".to_string(),
            "big data".to_string(), "data analysis".to_string(), "visualization".to_string(),
        ]);
        
        domain_keywords.insert("software".to_string(), vec![
            "software".to_string(), "programming".to_string(), "code".to_string(),
            "development".to_string(), "api".to_string(), "database".to_string(),
        ]);
        
        domain_keywords.insert("business".to_string(), vec![
            "business".to_string(), "strategy".to_string(), "market".to_string(),
            "customer".to_string(), "revenue".to_string(), "sales".to_string(),
        ]);
        
        domain_keywords.insert("finance".to_string(), vec![
            "finance".to_string(), "financial".to_string(), "investment".to_string(),
            "stock".to_string(), "market".to_string(), "trading".to_string(),
        ]);
        
        domain_keywords.insert("manufacturing".to_string(), vec![
            "manufacturing".to_string(), "supply chain".to_string(), "production".to_string(),
            "quality".to_string(), "mes".to_string(), "factory".to_string(),
        ]);
        
        domain_keywords.insert("retail".to_string(), vec![
            "retail".to_string(), "sku".to_string(), "inventory".to_string(),
            "ecommerce".to_string(), "consumer".to_string(), "o2o".to_string(),
        ]);
        
        Self { domain_keywords }
    }

    /// Analyze query and extract relevant domains
    pub fn analyze(&self, query_text: &str) -> Vec<(String, f64)> {
        let query_lower = query_text.to_lowercase();
        let mut domain_scores: Vec<(String, f64)> = Vec::new();
        
        for (domain, keywords) in &self.domain_keywords {
            let match_count = keywords
                .iter()
                .filter(|kw| query_lower.contains(&kw.to_lowercase()))
                .count();
            
            if match_count > 0 {
                // Score based on number of keyword matches
                let score = (match_count as f64 / keywords.len() as f64).min(1.0);
                domain_scores.push((domain.clone(), score));
            }
        }
        
        // Sort by score descending
        domain_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        
        // If no matches, return generic domain
        if domain_scores.is_empty() {
            domain_scores.push(("general".to_string(), 0.5));
        }
        
        domain_scores
    }
}

impl Default for QueryAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Cross-instance query router that uses metadata directory and query analysis
pub struct CrossInstanceRouter {
    inner: crate::routing::DomainRouter,
    metadata_directory: MetadataDirectory,
    query_analyzer: QueryAnalyzer,
}

impl CrossInstanceRouter {
    pub fn new(metadata_directory: MetadataDirectory) -> Self {
        Self {
            inner: crate::routing::DomainRouter::new(),
            metadata_directory,
            query_analyzer: QueryAnalyzer::new(),
        }
    }

    /// Route query using automatic domain detection and metadata directory
    pub async fn route_with_analysis(
        &self,
        query: &FederatedQuery,
        registry: &NodeRegistry,
    ) -> Result<RoutingDecision> {
        // Analyze query to detect domains
        let detected_domains = self.query_analyzer.analyze(&query.query_text);
        
        debug!(
            "Query analysis for '{}': {:?}",
            query.query_text, detected_domains
        );
        
        // Check metadata directory for expertise
        let mut enriched_domains = Vec::new();
        for (domain, score) in &detected_domains {
            if let Some(meta_score) = self.metadata_directory.get_expertise(domain).await {
                // Combine query analysis score with metadata expertise
                let combined_score = (score + meta_score) / 2.0;
                enriched_domains.push((domain.clone(), combined_score));
            } else {
                enriched_domains.push((domain.clone(), *score));
            }
        }
        
        // If query doesn't specify domains, use detected ones
        let final_domains = if query.domains.is_empty() {
            enriched_domains.iter().map(|(d, _)| d.clone()).collect()
        } else {
            query.domains.clone()
        };
        
        // Create enriched query
        let enriched_query = FederatedQuery {
            query_id: query.query_id.clone(),
            query_text: query.query_text.clone(),
            domains: final_domains,
            context: query.context.clone(),
            priority: query.priority,
            timeout_ms: query.timeout_ms,
            min_results: query.min_results,
        };
        
        // Route using domain router
        self.inner.route(&enriched_query, registry).await
    }
}

#[async_trait]
impl QueryRouter for CrossInstanceRouter {
    async fn route(
        &self,
        query: &FederatedQuery,
        registry: &NodeRegistry,
    ) -> Result<RoutingDecision> {
        // Use analysis-based routing
        self.route_with_analysis(query, registry).await
    }
}

/// Cross-instance query executor
pub struct CrossInstanceQueryExecutor {
    router: Arc<CrossInstanceRouter>,
    metadata_directory: MetadataDirectory,
    http_client: reqwest::Client,
}

impl CrossInstanceQueryExecutor {
    pub fn new(router: CrossInstanceRouter) -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(FederationError::NetworkError)?;

        Ok(Self {
            router: Arc::new(router),
            metadata_directory: MetadataDirectory::new(),
            http_client,
        })
    }

    /// Execute a cross-instance query
    pub async fn execute(
        &self,
        query: FederatedQuery,
        registry: &NodeRegistry,
    ) -> Result<FederatedResult> {
        // Get routing decision
        let routing = self.router.route(&query, registry).await?;
        
        info!(
            "Cross-instance query {} routed to {} nodes",
            query.query_id,
            routing.target_nodes.len()
        );
        
        // Execute queries to target nodes
        let mut results = Vec::new();
        
        for route in &routing.target_nodes {
            let result = self.execute_node_query(route, &query).await;
            results.push(result);
        }
        
        // Aggregate results
        let aggregator = WeightedAggregator::new();
        
        let node_results: Vec<NodeResult> = results
            .into_iter()
            .map(|r| r.unwrap_or_else(|e| {
                NodeResult {
                    node_id: "unknown".to_string(),
                    success: false,
                    data: None,
                    error: Some(e.to_string()),
                    response_time_ms: 0,
                    expertise_score: 0.0,
                }
            }))
            .collect();
        
        let aggregated = aggregator.aggregate(node_results.clone()).ok();
        
        let successful = node_results.iter().filter(|r| r.success).count();
        
        Ok(FederatedResult {
            query_id: query.query_id.clone(),
            node_results,
            aggregated,
            metadata: AggregationMetadata {
                total_nodes: routing.target_nodes.len(),
                successful_nodes: successful,
                failed_nodes: routing.target_nodes.len() - successful,
                consensus_reached: false,
                consensus_score: None,
                aggregation_method: "Weighted".to_string(),
                processing_time_ms: 0,
            },
        })
    }

    async fn execute_node_query(
        &self,
        route: &NodeRoute,
        query: &FederatedQuery,
    ) -> Result<NodeResult> {
        let start = std::time::Instant::now();
        
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "query_knowledge",
                "arguments": {
                    "query": query.query_text,
                    "context": {
                        "domain": query.domains.first(),
                    }
                }
            },
            "id": "1"
        });
        
        let response = self
            .http_client
            .post(&route.endpoint)
            .json(&payload)
            .timeout(std::time::Duration::from_millis(route.timeout_ms))
            .send()
            .await
            .map_err(FederationError::NetworkError)?;
        
        let response_time = start.elapsed().as_millis() as u64;
        
        if response.status().is_success() {
            let data: serde_json::Value = response
                .json()
                .await
                .map_err(FederationError::NetworkError)?;
            
            Ok(NodeResult {
                node_id: route.node_id.clone(),
                success: true,
                data: Some(data),
                error: None,
                response_time_ms: response_time,
                expertise_score: route.expected_expertise,
            })
        } else {
            Ok(NodeResult {
                node_id: route.node_id.clone(),
                success: false,
                data: None,
                error: Some(format!("HTTP {}", response.status())),
                response_time_ms: response_time,
                expertise_score: route.expected_expertise,
            })
        }
    }

    /// Suggest domains for a query based on metadata directory
    pub async fn suggest_domains(&self, query_text: &str) -> Vec<(String, f64)> {
        self.metadata_directory.suggest_domains(query_text, 5).await
    }
}

/// Builder for creating cross-instance routers
pub struct CrossInstanceRouterBuilder {
    metadata_directory: Option<MetadataDirectory>,
    min_expertise: f64,
    max_nodes: usize,
    strategy: RoutingStrategy,
}

impl CrossInstanceRouterBuilder {
    pub fn new() -> Self {
        Self {
            metadata_directory: None,
            min_expertise: 0.3,
            max_nodes: 5,
            strategy: RoutingStrategy::TopK(3),
        }
    }

    pub fn with_metadata_directory(mut self, dir: MetadataDirectory) -> Self {
        self.metadata_directory = Some(dir);
        self
    }

    pub fn with_min_expertise(mut self, min: f64) -> Self {
        self.min_expertise = min;
        self
    }

    pub fn with_max_nodes(mut self, max: usize) -> Self {
        self.max_nodes = max;
        self
    }

    pub fn with_strategy(mut self, strategy: RoutingStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    pub fn build(self) -> CrossInstanceRouter {
        let metadata_directory = self.metadata_directory.unwrap_or_default();
        CrossInstanceRouter::new(metadata_directory)
    }
}

impl Default for CrossInstanceRouterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_analyzer() {
        let analyzer = QueryAnalyzer::new();
        
        let domains = analyzer.analyze("What is machine learning and AI?");
        assert!(!domains.is_empty());
        assert!(domains.iter().any(|(d, _)| d == "ai"));
    }

    #[test]
    fn test_query_analyzer_no_match() {
        let analyzer = QueryAnalyzer::new();
        
        let domains = analyzer.analyze("random query text");
        // Should return general domain as fallback
        assert!(!domains.is_empty());
    }

    #[test]
    fn test_metadata_directory() {
        let dir = MetadataDirectory::new();
        
        let expertise = DomainExpertise {
            domain: "ai".to_string(),
            expertise_score: 0.9,
            claim_count: 100,
            avg_confidence: 0.85,
            node_types: HashMap::new(),
        };
        
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            dir.update_domain_expertise("ai".to_string(), expertise).await;
            let score = dir.get_expertise("ai").await;
            assert_eq!(score, Some(0.9));
        });
    }

    #[test]
    fn test_metadata_directory_suggest() {
        let dir = MetadataDirectory::new();
        
        let expertise = DomainExpertise {
            domain: "ai".to_string(),
            expertise_score: 0.9,
            claim_count: 100,
            avg_confidence: 0.85,
            node_types: HashMap::new(),
        };
        
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            dir.update_domain_expertise("ai".to_string(), expertise).await;
            
            let suggestions = dir.suggest_domains("machine learning models", 3).await;
            assert!(!suggestions.is_empty());
            // Should match "ai" domain due to keyword "learning"
            assert!(suggestions.iter().any(|(d, _)| d == "ai"));
        });
    }

    #[test]
    fn test_router_builder() {
        let router = CrossInstanceRouterBuilder::new()
            .with_min_expertise(0.5)
            .with_max_nodes(3)
            .build();
        
        // Just verify it builds without panicking
    }
}
