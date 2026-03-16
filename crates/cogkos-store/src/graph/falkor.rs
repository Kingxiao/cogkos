//! FalkorDB graph store implementation

use async_trait::async_trait;
use cogkos_core::models::{EpistemicClaim, GraphNode, Id};
use cogkos_core::{CogKosError, Result};
use redis::RedisError;
use uuid::Uuid;

/// Validate a UUID string to prevent Cypher injection.
/// FalkorDB does not support parameterized Cypher queries via GRAPH.QUERY,
/// so we must validate all interpolated values at the application layer.
pub(crate) fn validate_uuid(id: &impl std::fmt::Display) -> Result<String> {
    let s = id.to_string();
    if uuid::Uuid::parse_str(&s).is_ok() {
        Ok(s)
    } else {
        Err(CogKosError::InvalidInput(format!(
            "Invalid UUID for graph query: {}",
            s
        )))
    }
}

/// Escape a string value for Cypher single-quoted literals.
/// Escapes backslashes and single quotes.
pub(crate) fn cypher_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

/// Validate a relation name (only alphanumeric + underscore allowed).
pub(crate) fn validate_relation(rel: &str) -> Result<&str> {
    if !rel.is_empty() && rel.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        Ok(rel)
    } else {
        Err(CogKosError::InvalidInput(format!(
            "Invalid relation name: {}",
            rel
        )))
    }
}

/// FalkorDB (RedisGraph) store
pub struct FalkorStore {
    pool: deadpool_redis::Pool,
    graph_name: String,
}

impl FalkorStore {
    /// Create new Falkor store
    pub fn new(pool: deadpool_redis::Pool, graph_name: &str) -> Self {
        Self {
            pool,
            graph_name: graph_name.to_string(),
        }
    }

    /// Execute Cypher query
    async fn query(&self, cypher: &str) -> Result<Vec<redis::Value>> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| CogKosError::Graph(e.to_string()))?;

        let result: redis::Value = redis::cmd("GRAPH.QUERY")
            .arg(&self.graph_name)
            .arg(cypher)
            .query_async(&mut conn)
            .await
            .map_err(|e: RedisError| CogKosError::Graph(e.to_string()))?;

        // Parse result
        match result {
            redis::Value::Array(items) => Ok(items),
            _ => Ok(vec![]),
        }
    }
}

#[async_trait]
impl crate::GraphStore for FalkorStore {
    async fn add_node(&self, claim: &EpistemicClaim) -> Result<()> {
        let id = validate_uuid(&claim.id)?;
        let cypher = format!(
            "MERGE (c:Claim {{id: '{}', content: '{}', tenant_id: '{}', confidence: {}, activation: {}}})",
            id,
            cypher_escape(&claim.content),
            cypher_escape(&claim.tenant_id),
            claim.confidence,
            claim.activation_weight
        );

        self.query(&cypher).await?;
        Ok(())
    }

    async fn add_edge(&self, from: Id, to: Id, relation: &str, weight: f64) -> Result<()> {
        let from_id = validate_uuid(&from)?;
        let to_id = validate_uuid(&to)?;
        let rel = validate_relation(relation)?;
        let cypher = format!(
            "MATCH (a:Claim {{id: '{}'}}), (b:Claim {{id: '{}'}}) MERGE (a)-[r:{} {{weight: {}}}]->(b)",
            from_id, to_id, rel, weight
        );

        self.query(&cypher).await?;
        Ok(())
    }

    async fn find_related(
        &self,
        id: Id,
        depth: u32,
        min_activation: f64,
    ) -> Result<Vec<GraphNode>> {
        // Support CAUSES, SIMILAR_TO, DERIVED_FROM edge types for activation diffusion
        let safe_id = validate_uuid(&id)?;
        let cypher = format!(
            "MATCH (start:Claim {{id: '{}'}})-[:CAUSES|SIMILAR_TO|DERIVED_FROM*1..{}]->(related:Claim)
             WHERE related.activation >= {}
             RETURN related.id as id, related.content as content, related.activation as activation",
            safe_id, depth, min_activation
        );

        let results = self.query(&cypher).await?;
        let mut nodes = Vec::new();

        for result in results {
            if let redis::Value::Array(items) = result {
                // Parse result items
                for item in items {
                    if let redis::Value::Array(row) = item
                        && row.len() >= 3
                        && let (
                            redis::Value::BulkString(id_bytes),
                            redis::Value::BulkString(content_bytes),
                            redis::Value::BulkString(activation_bytes),
                        ) = (&row[0], &row[1], &row[2])
                        && let (Ok(id_str), Ok(content_str), Ok(activation_str)) = (
                            String::from_utf8(id_bytes.clone()),
                            String::from_utf8(content_bytes.clone()),
                            String::from_utf8(activation_bytes.clone()),
                        )
                        && let (Ok(id), Ok(activation)) =
                            (Uuid::parse_str(&id_str), activation_str.parse::<f64>())
                    {
                        nodes.push(GraphNode {
                            id,
                            content: content_str,
                            activation,
                        });
                    }
                }
            }
        }

        Ok(nodes)
    }

    async fn find_path(&self, from: Id, to: Id) -> Result<Vec<GraphNode>> {
        let from_id = validate_uuid(&from)?;
        let to_id = validate_uuid(&to)?;
        let cypher = format!(
            "MATCH path = shortestPath((a:Claim {{id: '{}'}})-[:RELATES_TO|IN_CONFLICT|DERIVED_FROM*]-(b:Claim {{id: '{}'}}))
             RETURN [node in nodes(path) | {{id: node.id, content: node.content, activation: node.activation}}] as path_nodes",
            from_id, to_id
        );

        let results = self.query(&cypher).await?;
        let nodes = Vec::new();

        for result in results {
            if let redis::Value::Array(items) = result {
                for item in items {
                    if let redis::Value::Array(row) = item {
                        // Parse path nodes
                        // This is a simplified version - real implementation would need proper parsing
                        for cell in row {
                            if let redis::Value::BulkString(data) = cell
                                && let Ok(json_str) = String::from_utf8(data)
                            {
                                // Parse JSON array of nodes
                                // Simplified for now
                                tracing::debug!("Path result: {}", json_str);
                            }
                        }
                    }
                }
            }
        }

        Ok(nodes)
    }

    async fn upsert_node(&self, claim: &EpistemicClaim) -> Result<()> {
        // Upsert is essentially add_node in FalkorDB
        self.add_node(claim).await
    }

    async fn create_edge(&self, from: Id, to: Id, relation: &str, weight: f64) -> Result<()> {
        self.add_edge(from, to, relation, weight).await
    }

    #[allow(clippy::collapsible_if, clippy::collapsible_match, clippy::len_zero)]
    async fn activation_diffusion(
        &self,
        start_id: Id,
        initial_activation: f64,
        depth: u32,
        decay_factor: f64,
        min_threshold: f64,
    ) -> Result<Vec<GraphNode>> {
        // BFS-based activation diffusion with initial_activation and decay_factor
        let mut activations: std::collections::HashMap<String, (f64, String)> =
            std::collections::HashMap::new();
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Initialize with starting node using initial_activation
        activations.insert(start_id.to_string(), (initial_activation, String::new()));

        // Default weights for relation types
        let get_relation_weight = |relation: &str| -> f64 {
            match relation {
                "CAUSES" => 0.8,
                "SIMILAR_TO" => 0.6,
                "DERIVED_FROM" => 0.7,
                "RELATED" => 0.4,
                "CONTRADICTS" | "IN_CONFLICT" => -0.3,
                _ => 0.5,
            }
        };

        // BFS propagation
        let mut current_level: std::collections::VecDeque<String> =
            std::collections::VecDeque::new();
        current_level.push_back(start_id.to_string());
        visited.insert(start_id.to_string());

        for _ in 0..depth {
            if current_level.is_empty() {
                break;
            }

            let mut next_level: std::collections::VecDeque<String> =
                std::collections::VecDeque::new();

            while let Some(current_id) = current_level.pop_front() {
                let current_activation = activations
                    .get(&current_id)
                    .map(|(act, _)| *act)
                    .unwrap_or(0.0);

                if current_activation < min_threshold {
                    continue;
                }

                // Find neighbors via CAUSES/SIMILAR_TO/DERIVED_FROM edges
                // current_id comes from previous Cypher results (validated UUID) or start_id (validated above)
                let safe_current_id = validate_uuid(&current_id)?;
                let neighbor_cypher = format!(
                    "MATCH (current:Claim {{id: '{}'}})-[r:CAUSES|SIMILAR_TO|DERIVED_FROM]->(neighbor:Claim)
                     RETURN neighbor.id as id, neighbor.content as content, type(r) as rel_type, r.weight as weight",
                    safe_current_id
                );

                let neighbor_results = self.query(&neighbor_cypher).await?;

                if let Some(result) = neighbor_results.first() {
                    if let redis::Value::Array(items) = result {
                        for item in items {
                            if let redis::Value::Array(row) = item
                                && row.len() >= 3
                                && let (
                                    redis::Value::BulkString(id_bytes),
                                    redis::Value::BulkString(content_bytes),
                                    redis::Value::BulkString(rel_bytes),
                                ) = (&row[0], &row[1], &row[2])
                                && let (Ok(id_str), Ok(content_str), Ok(rel_str)) = (
                                    String::from_utf8(id_bytes.clone()),
                                    String::from_utf8(content_bytes.clone()),
                                    String::from_utf8(rel_bytes.clone()),
                                )
                            {
                                // Get edge weight: use custom weight if > 0, otherwise use relation default
                                let edge_weight = if row.len() >= 4 {
                                    if let redis::Value::BulkString(weight_bytes) = &row[3] {
                                        if let Ok(weight_str) =
                                            String::from_utf8(weight_bytes.clone())
                                        {
                                            if let Ok(w) = weight_str.parse::<f64>() {
                                                if w > 0.0 {
                                                    w
                                                } else {
                                                    get_relation_weight(&rel_str)
                                                }
                                            } else {
                                                get_relation_weight(&rel_str)
                                            }
                                        } else {
                                            get_relation_weight(&rel_str)
                                        }
                                    } else {
                                        get_relation_weight(&rel_str)
                                    }
                                } else {
                                    get_relation_weight(&rel_str)
                                };

                                // Calculate new activation: current × edge_weight × decay_factor
                                let new_activation =
                                    current_activation * edge_weight * decay_factor;

                                if new_activation >= min_threshold {
                                    // Accumulate activation if already exists
                                    let existing = activations
                                        .get(&id_str)
                                        .map(|(act, _)| *act)
                                        .unwrap_or(0.0);

                                    activations.insert(
                                        id_str.clone(),
                                        (existing + new_activation, content_str.clone()),
                                    );

                                    if !visited.contains(&id_str) {
                                        visited.insert(id_str.clone());
                                        next_level.push_back(id_str);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            current_level = next_level;
        }

        // Get content for all visited nodes
        let mut result: Vec<GraphNode> = Vec::new();
        for (id_str, (activation, _)) in activations.iter() {
            if *activation >= min_threshold {
                // Query for content (id_str already validated by Uuid::parse_str above)
                let content_cypher = format!(
                    "MATCH (n:Claim {{id: '{}'}}) RETURN n.content as content",
                    validate_uuid(&id_str)?
                );

                let content_results = self.query(&content_cypher).await?;
                let content = if let Some(result) = content_results.first() {
                    if let redis::Value::Array(items) = result {
                        if let Some(item) = items.first() {
                            if let redis::Value::Array(row) = item
                                && row.len() >= 1
                                && let redis::Value::BulkString(content_bytes) = &row[0]
                            {
                                String::from_utf8(content_bytes.clone()).unwrap_or_default()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };

                if let Ok(id) = Uuid::parse_str(id_str) {
                    result.push(GraphNode {
                        id,
                        content,
                        activation: *activation,
                    });
                }
            }
        }

        Ok(result)
    }
}
