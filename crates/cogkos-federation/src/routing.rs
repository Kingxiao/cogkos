use crate::error::{FederationError, Result};
use crate::node::{FederatedNode, NodeRegistry};
use async_trait::async_trait;
use std::collections::HashMap;
use tracing::{debug, info, warn};

#[async_trait]
pub trait QueryRouter: Send + Sync {
    async fn route(
        &self,
        query: &FederatedQuery,
        registry: &NodeRegistry,
    ) -> Result<RoutingDecision>;
}

#[derive(Debug, Clone)]
pub struct FederatedQuery {
    pub query_id: String,
    pub query_text: String,
    pub domains: Vec<String>,
    pub context: HashMap<String, serde_json::Value>,
    pub priority: QueryPriority,
    pub timeout_ms: u64,
    pub min_results: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum QueryPriority {
    Low = 0,
    #[default]
    Normal = 1,
    High = 2,
    Critical = 3,
}

#[derive(Debug, Clone)]
pub struct RoutingDecision {
    pub query_id: String,
    pub target_nodes: Vec<NodeRoute>,
    pub strategy: RoutingStrategy,
    pub fallback_nodes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct NodeRoute {
    pub node_id: String,
    pub endpoint: String,
    pub expected_expertise: f64,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Copy)]
pub enum RoutingStrategy {
    SingleBest,                       // Route to single best node
    TopK(usize),                      // Route to top K nodes
    AllHealthy,                       // Route to all healthy nodes in domain
    Consensus { min_agreement: f64 }, // Need consensus across nodes
    ParallelAll,                      // Parallel to all matching nodes
}

impl Default for RoutingStrategy {
    fn default() -> Self {
        RoutingStrategy::TopK(3)
    }
}

pub struct DomainRouter {
    strategy: RoutingStrategy,
    min_expertise: f64,
    max_parallel: usize,
}

impl Default for DomainRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl DomainRouter {
    pub fn new() -> Self {
        Self {
            strategy: RoutingStrategy::TopK(3),
            min_expertise: 0.3,
            max_parallel: 5,
        }
    }

    pub fn with_strategy(mut self, strategy: RoutingStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    pub fn with_min_expertise(mut self, min: f64) -> Self {
        self.min_expertise = min.clamp(0.0, 1.0);
        self
    }

    fn select_nodes<'a>(
        &self,
        query: &FederatedQuery,
        registry: &'a NodeRegistry,
    ) -> Vec<&'a FederatedNode> {
        // Collect all healthy nodes that match any of the query domains
        let mut matching_nodes: Vec<&FederatedNode> = query
            .domains
            .iter()
            .flat_map(|domain| registry.healthy_nodes_for_domain(domain))
            .collect();

        // Remove duplicates
        matching_nodes.sort_by_key(|n| n.id.clone());
        matching_nodes.dedup_by_key(|n| n.id.clone());

        // Calculate composite expertise score for query domains
        let mut nodes_with_score: Vec<(&FederatedNode, f64)> = matching_nodes
            .into_iter()
            .map(|node| {
                let avg_expertise = if query.domains.is_empty() {
                    node.expertise_scores.values().sum::<f64>()
                        / node.expertise_scores.len().max(1) as f64
                } else {
                    query
                        .domains
                        .iter()
                        .map(|d| node.expertise_for(d))
                        .sum::<f64>()
                        / query.domains.len() as f64
                };
                (node, avg_expertise)
            })
            .filter(|(_, score)| *score >= self.min_expertise)
            .collect();

        // Sort by expertise (descending), then by priority (ascending)
        nodes_with_score.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap()
                .then_with(|| a.0.priority.cmp(&b.0.priority))
        });

        // Apply strategy
        let selected: Vec<&FederatedNode> = match self.strategy {
            RoutingStrategy::SingleBest => nodes_with_score
                .into_iter()
                .take(1)
                .map(|(n, _)| n)
                .collect(),
            RoutingStrategy::TopK(k) => nodes_with_score
                .into_iter()
                .take(k)
                .map(|(n, _)| n)
                .collect(),
            RoutingStrategy::AllHealthy | RoutingStrategy::ParallelAll => {
                nodes_with_score.into_iter().map(|(n, _)| n).collect()
            }
            RoutingStrategy::Consensus { .. } => {
                // For consensus, we want multiple nodes
                nodes_with_score
                    .into_iter()
                    .take(self.max_parallel)
                    .map(|(n, _)| n)
                    .collect()
            }
        };

        selected
    }
}

#[async_trait]
impl QueryRouter for DomainRouter {
    async fn route(
        &self,
        query: &FederatedQuery,
        registry: &NodeRegistry,
    ) -> Result<RoutingDecision> {
        debug!(
            "Routing query {} to domain {:?}",
            query.query_id, query.domains
        );

        let selected = self.select_nodes(query, registry);

        if selected.is_empty() {
            return Err(FederationError::RoutingError(format!(
                "No healthy nodes found for domains {:?}",
                query.domains
            )));
        }

        if selected.len() < query.min_results {
            warn!(
                "Only {} nodes available, but {} required",
                selected.len(),
                query.min_results
            );
        }

        // Get fallback nodes (next best nodes not in primary selection)
        let selected_ids: std::collections::HashSet<_> =
            selected.iter().map(|n| n.id.clone()).collect();

        let fallback: Vec<String> = registry
            .list_healthy()
            .into_iter()
            .filter(|n| !selected_ids.contains(&n.id))
            .take(2)
            .map(|n| n.id.clone())
            .collect();

        let routes: Vec<NodeRoute> = selected
            .iter()
            .map(|node| {
                let expertise = query
                    .domains
                    .iter()
                    .map(|d| node.expertise_for(d))
                    .sum::<f64>()
                    / query.domains.len().max(1) as f64;

                NodeRoute {
                    node_id: node.id.clone(),
                    endpoint: node.endpoint.clone(),
                    expected_expertise: expertise,
                    timeout_ms: query.timeout_ms,
                }
            })
            .collect();

        info!(
            "Routed query {} to {} nodes: {:?}",
            query.query_id,
            routes.len(),
            routes.iter().map(|r| &r.node_id).collect::<Vec<_>>()
        );

        Ok(RoutingDecision {
            query_id: query.query_id.clone(),
            target_nodes: routes,
            strategy: self.strategy,
            fallback_nodes: fallback,
        })
    }
}

pub struct MetadataDirectoryRouter {
    inner: DomainRouter,
}

impl Default for MetadataDirectoryRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataDirectoryRouter {
    pub fn new() -> Self {
        Self {
            inner: DomainRouter::new(),
        }
    }
}

#[async_trait]
impl QueryRouter for MetadataDirectoryRouter {
    async fn route(
        &self,
        query: &FederatedQuery,
        registry: &NodeRegistry,
    ) -> Result<RoutingDecision> {
        // First check if there's a metadata directory entry for exact domain match
        let exact_matches: Vec<&FederatedNode> = query
            .domains
            .iter()
            .flat_map(|domain| registry.healthy_nodes_for_domain(domain))
            .filter(|n| {
                n.expertise_scores
                    .iter()
                    .any(|(d, s)| query.domains.contains(d) && *s > 0.8)
            })
            .collect();

        if !exact_matches.is_empty() {
            debug!(
                "Found {} exact expert matches in metadata directory",
                exact_matches.len()
            );
        }

        // Fall back to domain router
        self.inner.route(query, registry).await
    }
}

/// Smart router that adapts based on query characteristics
pub struct AdaptiveRouter {
    _domain_router: DomainRouter,
    _min_nodes: usize,
    max_nodes: usize,
}

impl Default for AdaptiveRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl AdaptiveRouter {
    pub fn new() -> Self {
        Self {
            _domain_router: DomainRouter::new(),
            _min_nodes: 1,
            max_nodes: 5,
        }
    }

    pub fn determine_strategy(&self, query: &FederatedQuery) -> RoutingStrategy {
        match query.priority {
            QueryPriority::Critical => RoutingStrategy::ParallelAll,
            QueryPriority::High => RoutingStrategy::TopK(self.max_nodes.min(3)),
            QueryPriority::Normal => RoutingStrategy::TopK(2),
            QueryPriority::Low => RoutingStrategy::SingleBest,
        }
    }
}

#[async_trait]
impl QueryRouter for AdaptiveRouter {
    async fn route(
        &self,
        query: &FederatedQuery,
        registry: &NodeRegistry,
    ) -> Result<RoutingDecision> {
        let strategy = self.determine_strategy(query);

        let router = DomainRouter::new().with_strategy(strategy);

        router.route(query, registry).await
    }
}
