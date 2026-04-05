//! In-process graph cache for fast read-path queries.
//!
//! Wraps `InMemoryGraphStore` with startup warm-up from FalkorDB
//! and write-through synchronization.
//! Falls back to FalkorDB when the cache is cold or disabled.

use cogkos_core::models::{EpistemicClaim, GraphNode, Id};
use cogkos_core::{CogKosError, Result};
use std::collections::HashMap;
use std::sync::RwLock;
use tracing::{info, warn};

/// Maximum warm-up duration before auto-disabling.
const MAX_WARMUP_SECS: u64 = 30;

/// Maximum number of nodes before auto-disabling.
const MAX_NODES: usize = 500_000;

/// In-process graph cache.
///
/// Uses the same data structures as `InMemoryGraphStore` but is designed
/// to sit *in front of* FalkorDB rather than replace it.
pub struct GraphCache {
    nodes: RwLock<HashMap<Id, GraphNode>>,
    edges: RwLock<Vec<(Id, Id, String, f64)>>,
    enabled: bool,
}

impl GraphCache {
    /// Create an empty, enabled cache.
    pub fn new() -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
            edges: RwLock::new(Vec::new()),
            enabled: true,
        }
    }

    /// Create a disabled (no-op) cache.
    pub fn disabled() -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
            edges: RwLock::new(Vec::new()),
            enabled: false,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn node_count(&self) -> usize {
        self.nodes.read().map(|n| n.len()).unwrap_or(0)
    }

    pub fn edge_count(&self) -> usize {
        self.edges.read().map(|e| e.len()).unwrap_or(0)
    }

    /// Load graph data from FalkorDB into memory.
    pub async fn warm_from_falkor(pool: &deadpool_redis::Pool, graph_name: &str) -> Self {
        let start = std::time::Instant::now();

        // Load nodes
        let nodes = match Self::load_nodes(pool, graph_name).await {
            Ok(n) => n,
            Err(e) => {
                warn!(error = %e, "Graph cache: failed to load nodes, disabling");
                return Self::disabled();
            }
        };

        if nodes.len() > MAX_NODES {
            warn!(
                count = nodes.len(),
                "Graph cache: node count exceeds limit, disabling"
            );
            return Self::disabled();
        }

        if start.elapsed().as_secs() > MAX_WARMUP_SECS {
            warn!("Graph cache: warm-up exceeded 30s loading nodes, disabling");
            return Self::disabled();
        }

        // Load edges
        let edges = match Self::load_edges(pool, graph_name).await {
            Ok(e) => e,
            Err(e) => {
                warn!(error = %e, "Graph cache: failed to load edges, disabling");
                return Self::disabled();
            }
        };

        let elapsed = start.elapsed();
        if elapsed.as_secs() > MAX_WARMUP_SECS {
            warn!("Graph cache: warm-up exceeded 30s after loading edges, disabling");
            return Self::disabled();
        }

        info!(
            nodes = nodes.len(),
            edges = edges.len(),
            elapsed_ms = elapsed.as_millis() as u64,
            "Graph cache warmed from FalkorDB"
        );

        Self {
            nodes: RwLock::new(nodes),
            edges: RwLock::new(edges),
            enabled: true,
        }
    }

    async fn load_nodes(
        pool: &deadpool_redis::Pool,
        graph_name: &str,
    ) -> Result<HashMap<Id, GraphNode>> {
        let mut conn = pool
            .get()
            .await
            .map_err(|e| CogKosError::Graph(format!("pool error: {}", e)))?;

        let result: redis::Value = redis::cmd("GRAPH.QUERY")
            .arg(graph_name)
            .arg("MATCH (n:Claim) RETURN n.id, n.content, n.activation")
            .query_async(&mut conn)
            .await
            .map_err(|e| CogKosError::Graph(e.to_string()))?;

        let mut map = HashMap::new();
        if let redis::Value::Array(items) = result {
            for item in items {
                if let redis::Value::Array(rows) = item {
                    for row in rows {
                        if let redis::Value::Array(cols) = row
                            && cols.len() >= 3
                            && let (
                                redis::Value::BulkString(id_bytes),
                                redis::Value::BulkString(content_bytes),
                                redis::Value::BulkString(act_bytes),
                            ) = (&cols[0], &cols[1], &cols[2])
                            && let (Ok(id_str), Ok(content), Ok(act_str)) = (
                                String::from_utf8(id_bytes.clone()),
                                String::from_utf8(content_bytes.clone()),
                                String::from_utf8(act_bytes.clone()),
                            )
                            && let (Ok(id), Ok(activation)) =
                                (uuid::Uuid::parse_str(&id_str), act_str.parse::<f64>())
                        {
                            map.insert(
                                id,
                                GraphNode {
                                    id,
                                    content,
                                    activation,
                                },
                            );
                        }
                    }
                }
            }
        }
        Ok(map)
    }

    async fn load_edges(
        pool: &deadpool_redis::Pool,
        graph_name: &str,
    ) -> Result<Vec<(Id, Id, String, f64)>> {
        let mut conn = pool
            .get()
            .await
            .map_err(|e| CogKosError::Graph(format!("pool error: {}", e)))?;

        let result: redis::Value = redis::cmd("GRAPH.QUERY")
            .arg(graph_name)
            .arg(
                "MATCH (a:Claim)-[r]->(b:Claim) \
                 RETURN a.id, b.id, type(r), r.weight",
            )
            .query_async(&mut conn)
            .await
            .map_err(|e| CogKosError::Graph(e.to_string()))?;

        let mut edges = Vec::new();
        if let redis::Value::Array(items) = result {
            for item in items {
                if let redis::Value::Array(rows) = item {
                    for row in rows {
                        if let redis::Value::Array(cols) = row
                            && cols.len() >= 4
                            && let (
                                redis::Value::BulkString(from_bytes),
                                redis::Value::BulkString(to_bytes),
                                redis::Value::BulkString(rel_bytes),
                                redis::Value::BulkString(weight_bytes),
                            ) = (&cols[0], &cols[1], &cols[2], &cols[3])
                            && let (Ok(from_str), Ok(to_str), Ok(rel), Ok(weight_str)) = (
                                String::from_utf8(from_bytes.clone()),
                                String::from_utf8(to_bytes.clone()),
                                String::from_utf8(rel_bytes.clone()),
                                String::from_utf8(weight_bytes.clone()),
                            )
                            && let (Ok(from), Ok(to)) = (
                                uuid::Uuid::parse_str(&from_str),
                                uuid::Uuid::parse_str(&to_str),
                            )
                        {
                            let weight = weight_str.parse::<f64>().unwrap_or(0.5);
                            edges.push((from, to, rel, weight));
                        }
                    }
                }
            }
        }
        Ok(edges)
    }

    // ---- Read operations (served from memory) ----

    /// Find related nodes via BFS (same algorithm as InMemoryGraphStore).
    pub fn find_related(
        &self,
        id: Id,
        _tenant_id: &str,
        depth: u32,
        min_activation: f64,
    ) -> Option<Vec<GraphNode>> {
        if !self.enabled {
            return None;
        }

        let nodes = self.nodes.read().ok()?;
        let edges = self.edges.read().ok()?;

        let mut visited = std::collections::HashSet::new();
        let mut result = Vec::new();
        let mut queue = vec![(id, 0u32)];

        while let Some((current_id, current_depth)) = queue.pop() {
            if current_depth > depth || visited.contains(&current_id) {
                continue;
            }
            visited.insert(current_id);

            for (from, to, _, _) in edges.iter() {
                let neighbor = if *from == current_id {
                    *to
                } else if *to == current_id {
                    *from
                } else {
                    continue;
                };

                if let Some(node) = nodes.get(&neighbor) {
                    if node.activation >= min_activation && !visited.contains(&neighbor) {
                        if neighbor != id {
                            result.push(node.clone());
                        }
                        if current_depth < depth {
                            queue.push((neighbor, current_depth + 1));
                        }
                    }
                }
            }
        }

        Some(result)
    }

    /// Activation diffusion via BFS (same algorithm as InMemoryGraphStore).
    #[allow(clippy::too_many_arguments)]
    pub fn activation_diffusion(
        &self,
        start_id: Id,
        _tenant_id: &str,
        initial_activation: f64,
        depth: u32,
        decay_factor: f64,
        min_threshold: f64,
    ) -> Option<Vec<GraphNode>> {
        if !self.enabled {
            return None;
        }

        let nodes = self.nodes.read().ok()?;
        let edges = self.edges.read().ok()?;

        let get_relation_weight = |relation: &str| -> f64 {
            match relation {
                // Structural relations
                "CAUSES" => 0.8,
                "SIMILAR_TO" => 0.6,
                "DERIVED_FROM" => 0.7,
                "RELATED" | "SIMILAR" => 0.4,
                // Entity-mention relations
                "MENTIONS_PERSON" | "MENTIONS_DATE" | "MENTIONS_PLACE" | "MENTIONS_ORG" => 0.7,
                // Conflict relations
                "CONTRADICTS" | "IN_CONFLICT" => -0.3,
                // Precise triple-extracted relations
                "RESEARCHED" | "ATTENDED" | "PLANS_TO" => 0.7,
                "IS_A" | "WORKS_AS" => 0.6,
                "LIVES_IN" | "MOVED_FROM" => 0.6,
                "DOES_ACTIVITY" | "FEELS_ABOUT" => 0.5,
                "HAS_FAMILY" | "HAS_CHILD" | "HAS_PARTNER" => 0.8,
                // Default for any unknown relation type (future-proof)
                _ => 0.5,
            }
        };

        let mut activations: HashMap<Id, f64> = HashMap::new();
        activations.insert(start_id, initial_activation);

        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back((start_id, 0u32));

        while let Some((current_id, current_depth)) = queue.pop_front() {
            if current_depth >= depth || visited.contains(&current_id) {
                continue;
            }

            let current_activation = *activations.get(&current_id).unwrap_or(&0.0);
            if current_activation < min_threshold {
                continue;
            }
            visited.insert(current_id);

            for (from, to, relation, custom_weight) in edges.iter() {
                let neighbor = if *from == current_id {
                    *to
                } else if *to == current_id {
                    *from
                } else {
                    continue;
                };

                let edge_weight = if *custom_weight > 0.0 {
                    *custom_weight
                } else {
                    get_relation_weight(relation)
                };
                let new_activation = current_activation * edge_weight * decay_factor;

                if new_activation >= min_threshold {
                    let existing = *activations.get(&neighbor).unwrap_or(&0.0);
                    activations.insert(neighbor, existing + new_activation);

                    if !visited.contains(&neighbor) {
                        queue.push_back((neighbor, current_depth + 1));
                    }
                }
            }
        }

        let result: Vec<GraphNode> = activations
            .into_iter()
            .filter(|(id, act)| *act >= min_threshold && *id != start_id)
            .filter_map(|(id, activation)| {
                nodes.get(&id).map(|node| GraphNode {
                    id: node.id,
                    content: node.content.clone(),
                    activation,
                })
            })
            .collect();

        Some(result)
    }

    // ---- Write-through operations ----

    /// Add or update a node in the cache (call after FalkorDB write).
    pub fn upsert_node(&self, claim: &EpistemicClaim) {
        if !self.enabled {
            return;
        }
        if let Ok(mut nodes) = self.nodes.write() {
            nodes.insert(
                claim.id,
                GraphNode {
                    id: claim.id,
                    content: claim.content.clone(),
                    activation: claim.activation_weight,
                },
            );
        }
    }

    /// Add an edge to the cache (call after FalkorDB write).
    pub fn add_edge(&self, from: Id, to: Id, relation: &str, weight: f64) {
        if !self.enabled {
            return;
        }
        if let Ok(mut edges) = self.edges.write() {
            edges.push((from, to, relation.to_string(), weight));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_claim(id: Id, content: &str, activation: f64) -> EpistemicClaim {
        use cogkos_core::models::*;
        let prov =
            ProvenanceRecord::new("test".to_string(), "test".to_string(), "test".to_string());
        let mut claim = EpistemicClaim::new(
            content.to_string(),
            "test".to_string(),
            NodeType::Entity,
            Claimant::System,
            AccessEnvelope::new("test"),
            prov,
        );
        claim.id = id;
        claim.activation_weight = activation;
        claim
    }

    #[test]
    fn test_find_related_basic() {
        let cache = GraphCache::new();
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        let id3 = uuid::Uuid::new_v4();

        cache.upsert_node(&make_claim(id1, "a", 1.0));
        cache.upsert_node(&make_claim(id2, "b", 1.0));
        cache.upsert_node(&make_claim(id3, "c", 1.0));
        cache.add_edge(id1, id2, "RELATED", 0.5);
        cache.add_edge(id2, id3, "RELATED", 0.5);

        let related = cache.find_related(id1, "test", 2, 0.0).unwrap();
        assert_eq!(related.len(), 2);
    }

    #[test]
    fn test_find_related_min_activation() {
        let cache = GraphCache::new();
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();

        cache.upsert_node(&make_claim(id1, "a", 1.0));
        cache.upsert_node(&make_claim(id2, "b", 0.1));
        cache.add_edge(id1, id2, "RELATED", 0.5);

        let related = cache.find_related(id1, "test", 1, 0.5).unwrap();
        assert!(related.is_empty());
    }

    #[test]
    fn test_disabled_returns_none() {
        let cache = GraphCache::disabled();
        assert!(
            cache
                .find_related(uuid::Uuid::new_v4(), "t", 1, 0.0)
                .is_none()
        );
        assert!(
            cache
                .activation_diffusion(uuid::Uuid::new_v4(), "t", 1.0, 2, 0.8, 0.1)
                .is_none()
        );
    }

    #[test]
    fn test_activation_diffusion() {
        let cache = GraphCache::new();
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();

        cache.upsert_node(&make_claim(id1, "a", 1.0));
        cache.upsert_node(&make_claim(id2, "b", 1.0));
        cache.add_edge(id1, id2, "CAUSES", 0.8);

        let result = cache
            .activation_diffusion(id1, "test", 1.0, 2, 0.9, 0.1)
            .unwrap();
        assert_eq!(result.len(), 1);
        // 1.0 * 0.8 * 0.9 = 0.72
        assert!((result[0].activation - 0.72).abs() < 1e-6);
    }
}
