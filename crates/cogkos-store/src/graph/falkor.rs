//! FalkorDB graph store implementation

use async_trait::async_trait;
use cogkos_core::models::{EpistemicClaim, GraphNode, Id};
use cogkos_core::{CogKosError, Result};
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
///
/// When an in-process `GraphCache` is attached via `set_cache()`,
/// read-path operations (`find_related`, `activation_diffusion`) are
/// served from memory. Write operations write-through to both FalkorDB
/// and the cache. Falls back to FalkorDB if the cache is unavailable.
pub struct FalkorStore {
    pool: deadpool_redis::Pool,
    graph_name: String,
    cache: std::sync::RwLock<Option<crate::graph_cache::GraphCache>>,
}

impl FalkorStore {
    /// Create new Falkor store
    pub fn new(pool: deadpool_redis::Pool, graph_name: &str) -> Self {
        Self {
            pool,
            graph_name: graph_name.to_string(),
            cache: std::sync::RwLock::new(None),
        }
    }

    /// Attach an in-process graph cache for fast reads.
    pub fn set_cache(&self, cache: crate::graph_cache::GraphCache) {
        if let Ok(mut slot) = self.cache.write() {
            *slot = Some(cache);
        }
    }

    /// Execute Cypher query with retry (3 attempts, exponential backoff)
    async fn query(&self, cypher: &str) -> Result<Vec<redis::Value>> {
        let mut last_err = None;
        for attempt in 0..3u32 {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(100 * 2u64.pow(attempt))).await;
            }

            let conn = match self.pool.get().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(attempt, error = %e, "FalkorDB pool.get() failed, retrying");
                    last_err = Some(CogKosError::Graph(e.to_string()));
                    continue;
                }
            };
            let mut conn = conn;

            match redis::cmd("GRAPH.QUERY")
                .arg(&self.graph_name)
                .arg(cypher)
                .query_async::<redis::Value>(&mut conn)
                .await
            {
                Ok(redis::Value::Array(items)) => return Ok(items),
                Ok(_) => return Ok(vec![]),
                Err(e) => {
                    tracing::warn!(attempt, error = %e, "FalkorDB query failed, retrying");
                    last_err = Some(CogKosError::Graph(e.to_string()));
                }
            }
        }
        Err(last_err
            .unwrap_or_else(|| CogKosError::Graph("FalkorDB query exhausted retries".into())))
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

        // Write-through: sync cache.
        if let Ok(guard) = self.cache.read() {
            if let Some(cache) = guard.as_ref() {
                cache.upsert_node(claim);
            }
        }

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

        // Write-through: sync cache.
        if let Ok(guard) = self.cache.read() {
            if let Some(cache) = guard.as_ref() {
                cache.add_edge(from, to, relation, weight);
            }
        }

        Ok(())
    }

    async fn find_related(
        &self,
        id: Id,
        tenant_id: &str,
        depth: u32,
        min_activation: f64,
    ) -> Result<Vec<GraphNode>> {
        // Try in-process cache first.
        if let Ok(guard) = self.cache.read() {
            if let Some(cache) = guard.as_ref() {
                if let Some(results) = cache.find_related(id, tenant_id, depth, min_activation) {
                    return Ok(results);
                }
            }
        }

        // Fallback: FalkorDB query.
        let safe_id = validate_uuid(&id)?;
        let safe_tenant = cypher_escape(tenant_id);
        let cypher = format!(
            "MATCH (start:Claim {{id: '{}'}})-[*1..{}]-(related:Claim)
             WHERE related.activation >= {} AND related.tenant_id = '{}' AND related.id <> start.id
             RETURN DISTINCT related.id as id, related.content as content, related.activation as activation",
            safe_id, depth, min_activation, safe_tenant
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
            "MATCH path = shortestPath((a:Claim {{id: '{}'}})-[*]-(b:Claim {{id: '{}'}}))
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
        // add_node already handles write-through to cache
        self.add_node(claim).await
    }

    async fn create_edge(&self, from: Id, to: Id, relation: &str, weight: f64) -> Result<()> {
        // add_edge already handles write-through to cache
        self.add_edge(from, to, relation, weight).await
    }

    #[allow(clippy::collapsible_if, clippy::collapsible_match, clippy::len_zero)]
    async fn activation_diffusion(
        &self,
        start_id: Id,
        tenant_id: &str,
        initial_activation: f64,
        depth: u32,
        decay_factor: f64,
        min_threshold: f64,
    ) -> Result<Vec<GraphNode>> {
        // Try in-process cache first.
        if let Ok(guard) = self.cache.read() {
            if let Some(cache) = guard.as_ref() {
                if let Some(results) = cache.activation_diffusion(
                    start_id,
                    tenant_id,
                    initial_activation,
                    depth,
                    decay_factor,
                    min_threshold,
                ) {
                    return Ok(results);
                }
            }
        }

        // Fallback: FalkorDB BFS.
        let safe_tenant = cypher_escape(tenant_id);
        // BFS-based activation diffusion with initial_activation and decay_factor
        let mut activations: std::collections::HashMap<String, (f64, String)> =
            std::collections::HashMap::new();
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Initialize with starting node using initial_activation
        activations.insert(start_id.to_string(), (initial_activation, String::new()));

        // Default weights for relation types
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

                // Find neighbors via structural + entity-mention edges
                // current_id comes from previous Cypher results (validated UUID) or start_id (validated above)
                let safe_current_id = validate_uuid(&current_id)?;
                let neighbor_cypher = format!(
                    "MATCH (current:Claim {{id: '{}'}})-[r]-(neighbor:Claim)
                     WHERE neighbor.tenant_id = '{}' AND neighbor.id <> current.id
                     RETURN neighbor.id as id, neighbor.content as content, type(r) as rel_type, r.weight as weight",
                    safe_current_id, safe_tenant
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
                                // Non-propagating relations: skip regardless of custom weight
                                let default_weight = get_relation_weight(&rel_str);
                                if default_weight < 0.0 {
                                    continue;
                                }

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
