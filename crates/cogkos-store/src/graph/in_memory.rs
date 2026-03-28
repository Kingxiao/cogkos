//! In-memory graph store for testing

use async_trait::async_trait;
use cogkos_core::models::{EpistemicClaim, GraphNode, Id};
use cogkos_core::{CogKosError, Result};
use lru::LruCache;

/// In-memory graph store for testing
pub struct InMemoryGraphStore {
    nodes: std::sync::RwLock<std::collections::HashMap<Id, GraphNode>>,
    edges: std::sync::RwLock<Vec<(Id, Id, String, f64)>>, // from, to, relation, weight
    _diffusion_cache: std::sync::Arc<tokio::sync::RwLock<LruCache<Id, Vec<GraphNode>>>>,
}

impl InMemoryGraphStore {
    pub fn new() -> Self {
        let cache_capacity = std::num::NonZeroUsize::new(1000).unwrap();
        Self {
            nodes: std::sync::RwLock::new(std::collections::HashMap::new()),
            edges: std::sync::RwLock::new(Vec::new()),
            _diffusion_cache: std::sync::Arc::new(tokio::sync::RwLock::new(LruCache::new(
                cache_capacity,
            ))),
        }
    }
}

impl Default for InMemoryGraphStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl crate::GraphStore for InMemoryGraphStore {
    async fn add_node(&self, claim: &EpistemicClaim) -> Result<()> {
        let mut nodes = self
            .nodes
            .write()
            .map_err(|_| CogKosError::Internal("Lock poisoned".to_string()))?;
        nodes.insert(
            claim.id,
            GraphNode {
                id: claim.id,
                content: claim.content.clone(),
                activation: claim.activation_weight,
            },
        );
        Ok(())
    }

    async fn add_edge(&self, from: Id, to: Id, relation: &str, weight: f64) -> Result<()> {
        let mut edges = self
            .edges
            .write()
            .map_err(|_| CogKosError::Internal("Lock poisoned".to_string()))?;
        edges.push((from, to, relation.to_string(), weight));
        Ok(())
    }

    async fn find_related(
        &self,
        id: Id,
        _tenant_id: &str,
        depth: u32,
        min_activation: f64,
    ) -> Result<Vec<GraphNode>> {
        let nodes = self
            .nodes
            .read()
            .map_err(|_| CogKosError::Internal("Lock poisoned".to_string()))?;
        let edges = self
            .edges
            .read()
            .map_err(|_| CogKosError::Internal("Lock poisoned".to_string()))?;

        let mut visited = std::collections::HashSet::new();
        let mut result = Vec::new();
        let mut queue = vec![(id, 0)];

        while let Some((current_id, current_depth)) = queue.pop() {
            if current_depth > depth || visited.contains(&current_id) {
                continue;
            }
            visited.insert(current_id);

            // Find connected nodes
            for (from, to, _, _) in edges.iter() {
                let neighbor = if *from == current_id {
                    *to
                } else if *to == current_id {
                    *from
                } else {
                    continue;
                };

                if let Some(node) = nodes.get(&neighbor)
                    && node.activation >= min_activation
                    && !visited.contains(&neighbor)
                {
                    if neighbor != id {
                        result.push(node.clone());
                    }
                    if current_depth < depth {
                        queue.push((neighbor, current_depth + 1));
                    }
                }
            }
        }

        Ok(result)
    }

    async fn find_path(&self, from: Id, to: Id) -> Result<Vec<GraphNode>> {
        // Simple BFS for shortest path
        let nodes = self
            .nodes
            .read()
            .map_err(|_| CogKosError::Internal("Lock poisoned".to_string()))?;
        let edges = self
            .edges
            .read()
            .map_err(|_| CogKosError::Internal("Lock poisoned".to_string()))?;

        let mut visited = std::collections::HashSet::new();
        let mut came_from = std::collections::HashMap::new();
        let mut queue = vec![from];
        visited.insert(from);

        while let Some(current) = queue.pop() {
            if current == to {
                // Reconstruct path
                let mut path = vec![];
                let mut node = to;
                while let Some(prev) = came_from.get(&node) {
                    if let Some(n) = nodes.get(&node) {
                        path.push(n.clone());
                    }
                    node = *prev;
                }
                if let Some(n) = nodes.get(&from) {
                    path.push(n.clone());
                }
                path.reverse();
                return Ok(path);
            }

            // Find neighbors
            for (e_from, e_to, _, _) in edges.iter() {
                let neighbor = if *e_from == current {
                    *e_to
                } else if *e_to == current {
                    *e_from
                } else {
                    continue;
                };

                if !visited.contains(&neighbor) {
                    visited.insert(neighbor);
                    came_from.insert(neighbor, current);
                    queue.push(neighbor);
                }
            }
        }

        Ok(vec![]) // No path found
    }

    async fn upsert_node(&self, claim: &EpistemicClaim) -> Result<()> {
        // Upsert is essentially add_node in in-memory store
        self.add_node(claim).await
    }

    async fn create_edge(&self, from: Id, to: Id, relation: &str, weight: f64) -> Result<()> {
        self.add_edge(from, to, relation, weight).await
    }

    async fn activation_diffusion(
        &self,
        start_id: Id,
        _tenant_id: &str,
        initial_activation: f64,
        depth: u32,
        decay_factor: f64,
        min_threshold: f64,
    ) -> Result<Vec<GraphNode>> {
        let nodes = self
            .nodes
            .read()
            .map_err(|_| CogKosError::Internal("Lock poisoned".to_string()))?;
        let edges = self
            .edges
            .read()
            .map_err(|_| CogKosError::Internal("Lock poisoned".to_string()))?;

        // Initialize activation for each node
        let mut activations: std::collections::HashMap<Id, f64> = std::collections::HashMap::new();
        activations.insert(start_id, initial_activation);

        // BFS with activation propagation
        let mut visited = std::collections::HashSet::new();
        let mut queue = vec![(start_id, 0)];

        // Get default weights for relation types
        let get_relation_weight = |relation: &str| -> f64 {
            match relation {
                "CAUSES" => 0.8,
                "SIMILAR_TO" => 0.6,
                "DERIVED_FROM" => 0.7,
                "RELATED" => 0.4,
                "MENTIONS_PERSON" | "MENTIONS_DATE" | "MENTIONS_PLACE" | "MENTIONS_ORG" => 0.7,
                "CONTRADICTS" | "IN_CONFLICT" => -0.3,
                _ => 0.5,
            }
        };

        // Propagate along structural and entity-mention edges
        let allowed_relations: std::collections::HashSet<&str> = [
            "CAUSES",
            "SIMILAR_TO",
            "DERIVED_FROM",
            "MENTIONS_PERSON",
            "MENTIONS_DATE",
            "MENTIONS_PLACE",
            "MENTIONS_ORG",
        ]
        .iter()
        .cloned()
        .collect();

        while let Some((current_id, current_depth)) = queue.pop() {
            if current_depth >= depth {
                continue;
            }

            let current_activation = *activations.get(&current_id).unwrap_or(&0.0);
            if current_activation < min_threshold {
                continue;
            }

            if visited.contains(&current_id) {
                continue;
            }
            visited.insert(current_id);

            // Find all CAUSES/SIMILAR_TO edges from/to current node and propagate activation
            for (from, to, relation, custom_weight) in edges.iter() {
                // Only propagate along CAUSES and SIMILAR_TO edges
                if !allowed_relations.contains(relation.as_str()) {
                    continue;
                }

                let neighbor = if *from == current_id {
                    *to
                } else if *to == current_id {
                    *from
                } else {
                    continue;
                };

                // Calculate new activation: current × edge_weight × decay
                let edge_weight = if *custom_weight > 0.0 {
                    *custom_weight
                } else {
                    get_relation_weight(relation)
                };
                let new_activation = current_activation * edge_weight * decay_factor;

                // Only propagate if above threshold
                if new_activation >= min_threshold {
                    // Update neighbor's activation (accumulate if already visited)
                    let existing = activations.get(&neighbor).unwrap_or(&0.0);
                    activations.insert(neighbor, existing + new_activation);

                    if !visited.contains(&neighbor) {
                        queue.push((neighbor, current_depth + 1));
                    }
                }
            }
        }

        // Collect all nodes above threshold with their final activations
        let result: Vec<GraphNode> = activations
            .into_iter()
            .filter(|(_, activation)| *activation >= min_threshold)
            .filter(|(id, _)| *id != start_id)
            .filter_map(|(id, activation)| {
                nodes.get(&id).map(|node| GraphNode {
                    id: node.id,
                    content: node.content.clone(),
                    activation,
                })
            })
            .collect();

        Ok(result)
    }
}
