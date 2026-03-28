//! Query knowledge handler and related functions

use crate::merger::{MergeConfig, merge_results};
use crate::server::JsonRpcError;
use cogkos_core::Result;
use cogkos_core::models::*;
use cogkos_ingest::EmbeddingService;
use cogkos_llm::{LlmClient, LlmRequest, Message, Role};
use cogkos_store::{CacheStore, ClaimStore, GapStore, GraphStore, KnowledgeGapRecord, VectorStore};
use std::sync::Arc;
use std::time::Duration;

use super::helpers::{self, calculate_query_hash, generate_query_vector};
use super::types::*;

/// Maximum number of claims to run graph diffusion on.
/// Beyond this, marginal recall gain does not justify the FalkorDB round-trips.
const MAX_GRAPH_DIFFUSION_CLAIMS: usize = 5;

/// RRF boost factor for entity-constrained search results.
/// Higher = stronger preference for claims that mention query entities.
const ENTITY_RRF_BOOST: f64 = 1.5;

/// Extract proper nouns (entity names) from a query string.
///
/// Uses a simple heuristic: capitalized words that are not at sentence start
/// and not common English question/function words.
/// Tulving's encoding specificity principle: retrieval cues must match
/// the encoding context — entity names are the strongest cues.
fn extract_query_entities(query: &str) -> Vec<String> {
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut entities = Vec::new();

    // Common question/function words that happen to be capitalized at sentence start
    const SKIP: &[&str] = &[
        "What", "When", "Where", "Which", "Who", "How", "Does", "Did", "Is", "Are", "Was", "Were",
        "Has", "Have", "Would", "Could", "Should", "Can", "Will", "Do", "The", "A", "An", "In",
        "On", "At", "To", "For", "Of", "With", "By", "From", "About", "Into", "After", "Before",
        "During", "Between", "Through", "And", "Or", "But", "Not", "If", "Then", "So", "That",
        "This", "It", "They", "He", "She", "We", "You", "My", "Your", "His", "Her", "Its", "Our",
        "Their",
    ];

    for word in &words {
        let clean = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
        if clean.is_empty() || clean.len() <= 1 {
            continue;
        }

        // Must start with uppercase
        if !clean.chars().next().map_or(false, |c| c.is_uppercase()) {
            continue;
        }

        // Skip common function words
        if SKIP.contains(&clean) {
            continue;
        }

        // Skip ALL-CAPS words (likely acronyms in questions, e.g. "API", "SLA")
        // unless they're short enough to be names
        if clean.len() > 2 && clean.chars().all(|c| c.is_uppercase()) {
            continue;
        }

        entities.push(clean.to_string());
    }

    entities.dedup();
    entities
}

/// Query granularity — heuristic classification to boost results matching the
/// expected level of detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryGranularity {
    /// "When/Where/Who" questions — favour fine-grained episodic/working claims
    Precise,
    /// "What about/How does" questions — no layer preference
    Thematic,
    /// "What kind of/Describe/Summarize" questions — favour aggregated semantic claims
    General,
}

/// Detect query granularity from the raw query string.
fn detect_query_granularity(query: &str) -> QueryGranularity {
    let q = query.to_lowercase();

    // Precise: temporal, spatial, identity questions
    if q.starts_with("when ")
        || q.starts_with("where ")
        || q.starts_with("who ")
        || q.contains("what date")
        || q.contains("what time")
        || q.contains("how long ago")
        || q.contains("how old")
        || q.contains("what year")
        || q.contains("what month")
    {
        return QueryGranularity::Precise;
    }

    // General: personality, description, summary questions
    if q.starts_with("describe ")
        || q.starts_with("summarize ")
        || q.contains("what kind of")
        || q.contains("what type of")
        || q.contains("personality")
        || q.contains("character")
    {
        return QueryGranularity::General;
    }

    // Default: thematic
    QueryGranularity::Thematic
}

/// Score boost for a claim based on whether its memory_layer matches the query
/// granularity. Returns an additive bonus to the combined_score.
fn granularity_boost(granularity: QueryGranularity, claim: &EpistemicClaim) -> f64 {
    let layer = claim
        .metadata
        .get("memory_layer")
        .and_then(|v| v.as_str())
        .unwrap_or("semantic");

    match granularity {
        QueryGranularity::Precise => {
            if layer == "episodic" || layer == "working" {
                0.15
            } else {
                0.0
            }
        }
        QueryGranularity::General => {
            if layer == "semantic" {
                0.1
            } else {
                0.0
            }
        }
        QueryGranularity::Thematic => 0.0,
    }
}

/// Activation weight buffer — batches PG updates to reduce write pressure on the read path.
/// Flushed every `FLUSH_INTERVAL` by a background tokio task.
pub struct ActivationBuffer {
    inner: tokio::sync::Mutex<Vec<(uuid::Uuid, String, f64)>>,
}

impl ActivationBuffer {
    pub fn new() -> Self {
        Self {
            inner: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    /// Enqueue an activation delta (non-blocking: only contends with the flush task).
    pub async fn push(&self, claim_id: uuid::Uuid, tenant_id: &str, delta: f64) {
        self.inner
            .lock()
            .await
            .push((claim_id, tenant_id.to_owned(), delta));
    }

    /// Drain all pending updates and apply them.
    async fn flush(&self, claim_store: &dyn ClaimStore) {
        let updates = {
            let mut buf = self.inner.lock().await;
            std::mem::take(&mut *buf)
        };
        if updates.is_empty() {
            return;
        }
        tracing::debug!(count = updates.len(), "Flushing activation buffer");
        for (id, tenant, delta) in updates {
            claim_store.update_activation(id, &tenant, delta).await.ok();
        }
    }

    /// Spawn a background flush loop. Returns a `JoinHandle` the caller can
    /// abort on shutdown if desired.
    pub fn spawn_flush_loop(
        self: &Arc<Self>,
        claim_store: Arc<dyn ClaimStore>,
    ) -> tokio::task::JoinHandle<()> {
        let buf = Arc::clone(self);
        let flush_interval_secs: u64 = std::env::var("ACTIVATION_FLUSH_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(flush_interval_secs));
            loop {
                interval.tick().await;
                buf.flush(claim_store.as_ref()).await;
            }
        })
    }
}

/// Query knowledge handler
#[allow(clippy::too_many_arguments)]
pub async fn handle_query_knowledge(
    req: QueryKnowledgeRequest,
    tenant_id: &str,
    _roles: &[String],
    claim_store: &dyn ClaimStore,
    vector_store: &dyn VectorStore,
    graph_store: &dyn GraphStore,
    cache_store: &dyn CacheStore,
    gap_store: &dyn GapStore,
    llm_client: Option<Arc<dyn LlmClient>>,
    embedding_client: Option<Arc<dyn LlmClient>>,
    activation_buffer: Option<&Arc<ActivationBuffer>>,
) -> Result<McpQueryResponse> {
    let start_time = std::time::Instant::now();

    // Validate query is not empty
    if req.query.trim().is_empty() {
        return Ok(McpQueryResponse {
            query_hash: 0,
            query_context: req.query.clone(),
            best_belief: None,
            related_by_graph: vec![],
            conflicts: vec![],
            prediction: None,
            knowledge_gaps: vec![],
            freshness: FreshnessInfo {
                newest_source: None,
                oldest_source: None,
                staleness_warning: false,
            },
            cache_status: CacheStatus::Miss,
            cognitive_path: None,
            metadata: QueryMetadata {
                execution_time_ms: 0,
                cache_hit_rate: 0.0,
                processed_claims: 0,
                related_node_count: 0,
                conflict_count: 0,
            },
        });
    }

    // Calculate query hash for cache lookup
    let query_hash = calculate_query_hash(&req.query, &req.context.domain);

    // 1. Check cache — S6 dual-path decision (skip if high urgency)
    let dual_path = DualPathThresholds::from_env();
    if !matches!(req.context.urgency, Urgency::High)
        && let Some(cached) = cache_store.get_cached(tenant_id, query_hash).await?
    {
        let ttl_seconds = std::env::var("CACHE_TTL_SECONDS")
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(3600);

        if cached.is_valid(ttl_seconds) {
            // S6: Determine cognitive path
            if cached.qualifies_for_system1(&dual_path) {
                // === System 1: Fast path — high confidence, return immediately ===
                cache_store.record_hit(tenant_id, query_hash).await?;

                // S3 read-is-write: buffer activation updates for cached claims
                if let Some(ref belief) = cached.response.best_belief {
                    if let Some(buf) = activation_buffer {
                        for claim_id in &belief.claim_ids {
                            buf.push(*claim_id, tenant_id, 0.05).await;
                        }
                    } else {
                        for claim_id in &belief.claim_ids {
                            claim_store
                                .update_activation(*claim_id, tenant_id, 0.05)
                                .await
                                .ok();
                        }
                    }
                }

                let hit_rate = cached.success_rate();
                let mut response = cached.response.clone();
                response.cache_status = CacheStatus::Hit;
                response.cognitive_path = Some(CognitivePath::System1);
                response.metadata.execution_time_ms = start_time.elapsed().as_millis() as u64;
                response.metadata.cache_hit_rate = hit_rate;
                cogkos_core::monitoring::METRICS.inc_counter("cogkos_cache_hit_total", 1);

                tracing::debug!(
                    query_hash = query_hash,
                    confidence = cached.confidence,
                    hit_count = cached.hit_count,
                    success_rate = hit_rate,
                    "System 1 fast path: cache hit with high confidence"
                );

                return Ok(response);
            } else {
                // Cache exists but low confidence → degrade to System 2
                tracing::info!(
                    query_hash = query_hash,
                    confidence = cached.confidence,
                    success_rate = cached.success_rate(),
                    "System 2 triggered: cache confidence below threshold, running full reasoning"
                );
                // Fall through to System 2 full path
            }
        } else {
            tracing::debug!(
                query_hash = query_hash,
                confidence = cached.confidence,
                invalidated = cached.invalidated_by.is_some(),
                "Cache entry expired or invalid, using System 2"
            );
        }
    }

    // 2. Vector search - embed query using embedding service
    let query_vector = if let Some(ref client) = embedding_client {
        let embedding_service = EmbeddingService::new(client.clone());
        match embedding_service.embed(&req.query).await {
            Ok(vec) => vec,
            Err(e) => {
                tracing::warn!("Embedding failed, using fallback: {}", e);
                generate_query_vector(&req.query, helpers::DEFAULT_FALLBACK_DIM)
            }
        }
    } else {
        tracing::warn!("No embedding client configured, using fallback");
        generate_query_vector(&req.query, helpers::DEFAULT_FALLBACK_DIM)
    };
    // Default to semantic layer — prevents working/episodic memory from leaking
    // into queries that don't explicitly request them.
    // Agents share the semantic layer within the same tenant (shared brain).
    // Working/episodic require explicit memory_layer + session_id to access.
    let effective_layer = req.memory_layer.as_deref().or(Some("semantic"));

    let vector_matches = vector_store
        .search(
            query_vector,
            tenant_id,
            req.context.max_results,
            effective_layer,
        )
        .await?;

    // 2a. Similarity threshold gate — filter out irrelevant nearest neighbors
    let min_similarity: f64 = std::env::var("MIN_SIMILARITY_THRESHOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.5);

    let vector_matches: Vec<_> = vector_matches
        .into_iter()
        .filter(|m| m.score >= min_similarity)
        .collect();

    // 2b. Full-text keyword search — catches exact term matches that vectors miss
    let text_matches = claim_store
        .search_claims(tenant_id, &req.query, req.context.max_results as usize)
        .await
        .unwrap_or_default();

    // 2c. Entity-constrained retrieval (Tulving encoding specificity)
    //     Extract proper nouns from query, search for claims mentioning them.
    //     This is an additive 4th retrieval path — does NOT replace vector/text/graph.
    let query_entities = extract_query_entities(&req.query);
    let entity_matches = if !query_entities.is_empty() {
        let entity_query = query_entities.join(" ");
        tracing::debug!(
            entities = ?query_entities,
            entity_query = %entity_query,
            "Entity-constrained retrieval: searching for entity mentions"
        );
        claim_store
            .search_claims(tenant_id, &entity_query, 30) // larger pool — will be RRF-ranked
            .await
            .unwrap_or_default()
    } else {
        vec![]
    };

    // 2d. Merge via RRF (Reciprocal Rank Fusion) — k=60 is the standard constant
    let rrf_k = 60.0_f64;
    let mut rrf_scores: std::collections::HashMap<uuid::Uuid, f64> =
        std::collections::HashMap::new();

    for (rank, m) in vector_matches.iter().enumerate() {
        *rrf_scores.entry(m.id).or_default() += 1.0 / (rrf_k + rank as f64);
    }
    for (rank, c) in text_matches.iter().enumerate() {
        *rrf_scores.entry(c.id).or_default() += 1.0 / (rrf_k + rank as f64);
    }
    // Entity-constrained results get a boosted RRF contribution
    for (rank, c) in entity_matches.iter().enumerate() {
        *rrf_scores.entry(c.id).or_default() += ENTITY_RRF_BOOST / (rrf_k + rank as f64);
    }

    // Build a quick lookup of text-matched + entity-matched claims to avoid redundant DB round-trips
    let text_claim_map: std::collections::HashMap<uuid::Uuid, &EpistemicClaim> = text_matches
        .iter()
        .chain(entity_matches.iter())
        .map(|c| (c.id, c))
        .collect();

    // 3. Get claims from merged results with comprehensive permission filtering
    let mut claims = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    let mut claim_ids: Vec<uuid::Uuid> = Vec::new();

    // Sort merged IDs by descending RRF score, take top max_results
    let mut merged_ids: Vec<_> = rrf_scores.into_iter().collect();
    merged_ids.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    merged_ids.truncate(req.context.max_results as usize);

    for (id, _score) in &merged_ids {
        if !seen_ids.insert(*id) {
            continue;
        }
        // Prefer already-fetched text matches to avoid extra DB call
        if let Some(claim) = text_claim_map.get(id) {
            claim_ids.push(claim.id);
            claims.push((*claim).clone());
        } else if let Ok(claim) = claim_store.get_claim(*id, tenant_id).await {
            claim_ids.push(claim.id);
            claims.push(claim);
        }
    }

    // 3a-post. Filter out expired claims (cache hit may bypass SQL filter)
    claims.retain(|c| c.t_valid_end.map_or(true, |end| end > chrono::Utc::now()));
    claim_ids = claims.iter().map(|c| c.id).collect();

    // 3b. Post-filter by session_id if requested (for working memory scoping)
    if let Some(ref sid) = req.session_id {
        claims.retain(|c| {
            c.metadata
                .get("session_id")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s == sid)
        });
        claim_ids = claims.iter().map(|c| c.id).collect();
    }

    // 3c. Episodic memory isolation: filter by agent_id
    // Episodic memories belong to individual agents — Agent A's experiences
    // should not appear in Agent B's results unless explicitly shared (semantic layer).
    if effective_layer == Some("episodic") {
        if let Some(ref agent) = req.agent_id {
            claims.retain(|c| match &c.claimant {
                cogkos_core::models::Claimant::Agent { agent_id, .. } => agent_id == agent,
                _ => false,
            });
            claim_ids = claims.iter().map(|c| c.id).collect();
        }
    }

    // 3d. Namespace isolation — filter claims by namespace if requested.
    // Claims without a namespace are always visible (public knowledge).
    // Claims with a namespace must match the requested namespace exactly.
    if let Some(ref ns) = req.namespace {
        claims.retain(|c| {
            c.metadata
                .get("namespace")
                .and_then(|v| v.as_str())
                .map_or(true, |claim_ns| claim_ns == ns)
        });
        claim_ids = claims.iter().map(|c| c.id).collect();
    }

    // 3e. Record rehearsal for retrieved claims (S3: read equals write)
    for claim in &claims {
        let layer = cogkos_core::models::MemoryLayer::from_metadata(&claim.metadata);
        let delta = layer.lambda() * 0.4;
        if let Some(buf) = activation_buffer {
            buf.push(claim.id, tenant_id, delta).await;
        } else {
            claim_store
                .update_activation(claim.id, tenant_id, delta)
                .await
                .ok();
        }
    }

    // 4. Graph activation diffusion as independent retrieval path
    //    Use activation_diffusion (BFS with decay) instead of find_related,
    //    and expand seed set to ALL claims from vector+text search (not just top-5).
    //    Graph-discovered claims are added back to the main result set.
    let mut all_graph_nodes: Vec<GraphNode> = Vec::new();
    let threshold = req.activation_threshold.clamp(0.0, 1.0);
    let decay_factor = 0.8;
    let max_depth = 2;

    // Use all seed claim IDs (from vector + text search) for graph diffusion,
    // capped at MAX_GRAPH_DIFFUSION_CLAIMS sorted by RRF rank (already sorted).
    let diffusion_ids: Vec<uuid::Uuid> = claim_ids
        .iter()
        .take(MAX_GRAPH_DIFFUSION_CLAIMS)
        .copied()
        .collect();

    // Fire all graph activation_diffusion queries concurrently
    let graph_futures: Vec<_> = diffusion_ids
        .iter()
        .map(|&id| async move {
            match graph_store
                .activation_diffusion(id, tenant_id, 1.0, max_depth, decay_factor, threshold)
                .await
            {
                Ok(nodes) => (id, nodes),
                Err(e) => {
                    tracing::warn!(claim_id = %id, error = %e, "Graph diffusion failed, degrading");
                    (id, vec![])
                }
            }
        })
        .collect();

    let graph_results = futures::future::join_all(graph_futures).await;

    let seed_id_set: std::collections::HashSet<uuid::Uuid> = claim_ids.iter().copied().collect();

    for (_claim_id, related) in graph_results {
        for mut node in related {
            if all_graph_nodes.iter().any(|n: &GraphNode| n.id == node.id) {
                continue;
            }
            // Enrich with live activation from PG (FalkorDB stores stale initial value)
            // Also filter: only include nodes from the same memory layer as the query
            if let Ok(live_claim) = claim_store.get_claim(node.id, tenant_id).await {
                let node_layer = live_claim.memory_layer().to_string();
                let target_layer = effective_layer.unwrap_or("semantic");
                if node_layer != target_layer {
                    continue;
                }
                node.activation = live_claim.activation_weight;

                // Graph-discovered claims not in the seed set: add to main claims list
                if !seed_id_set.contains(&node.id) && seen_ids.insert(node.id) {
                    claim_ids.push(live_claim.id);
                    claims.push(live_claim);
                }
            }
            all_graph_nodes.push(node);
        }
    }

    // 5. Merge vector search results with graph diffusion results
    let merge_config = MergeConfig {
        max_results: req.context.max_results as usize,
        ..MergeConfig::default()
    };

    let claim_tuples: Vec<(uuid::Uuid, EpistemicClaim)> =
        claims.iter().map(|c| (c.id, c.clone())).collect();

    let (mut merged_results, related_by_graph) = merge_results(
        &vector_matches,
        &all_graph_nodes,
        &claim_tuples,
        &merge_config,
        &req.query,
    );

    // 5b. Apply query-granularity boost — rewards claims whose memory_layer
    //     matches the expected level of detail for the query type.
    let granularity = detect_query_granularity(&req.query);
    for mr in &mut merged_results {
        if let Some(claim) = claims.iter().find(|c| c.id == mr.claim_id) {
            mr.combined_score += granularity_boost(granularity, claim);
        }
    }

    // 5c. Entity-presence boost (Tulving encoding specificity post-filter)
    //     Claims whose content contains query entities get a score bonus.
    //     This re-ranks without removing any results.
    if !query_entities.is_empty() {
        let entity_boost: f64 = std::env::var("ENTITY_PRESENCE_BOOST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.2);

        for mr in &mut merged_results {
            if let Some(claim) = claims.iter().find(|c| c.id == mr.claim_id) {
                let entity_hit_count = query_entities
                    .iter()
                    .filter(|e| claim.content.contains(e.as_str()))
                    .count();
                if entity_hit_count > 0 {
                    // Proportional boost: more entity matches = higher boost
                    let ratio = entity_hit_count as f64 / query_entities.len() as f64;
                    mr.combined_score += entity_boost * ratio;
                }
            }
        }
    }

    // Re-sort after boosting
    merged_results.sort_by(|a, b| {
        b.combined_score
            .partial_cmp(&a.combined_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 6. Find best belief from merged results
    let best_belief = if !merged_results.is_empty() {
        let best = merged_results.first();
        best.map(|r| {
            let claim = claims.iter().find(|c| c.id == r.claim_id);
            let reliability = claim.map(|c| {
                let tier = cogkos_core::authority::AuthorityTier::resolve(c);
                let has_positive_feedback = c.access_count >= 3 && c.confidence >= 0.7;
                let has_negative_feedback = c.confidence < 0.4;
                let is_contested = matches!(c.epistemic_status, EpistemicStatus::Contested);

                if has_positive_feedback
                    && matches!(
                        tier,
                        cogkos_core::authority::AuthorityTier::Canonical
                            | cogkos_core::authority::AuthorityTier::Curated
                            | cogkos_core::authority::AuthorityTier::Verified
                    )
                {
                    "high".to_string()
                } else if has_positive_feedback
                    || matches!(tier, cogkos_core::authority::AuthorityTier::Curated)
                {
                    "medium".to_string()
                } else if has_negative_feedback || is_contested {
                    "low".to_string()
                } else {
                    "unverified".to_string()
                }
            });

            BeliefSummary {
                claim_id: Some(r.claim_id),
                content: strip_yaml_frontmatter(&r.content),
                confidence: r.confidence,
                based_on: claims.len(),
                consolidation_stage: claim
                    .map(|c| c.consolidation_stage)
                    .unwrap_or(ConsolidationStage::FastTrack),
                claim_ids: merged_results.iter().map(|r| r.claim_id).collect(),
                reliability,
            }
        })
    } else {
        None
    };

    // 7. Detect conflicts
    let mut conflicts = Vec::new();
    let mut sampling_conflicts = Vec::new();
    if req.include_conflicts {
        let mut seen_conflicts = std::collections::HashSet::new();
        for claim in &claims {
            let claim_conflicts = match claim_store
                .get_conflicts_for_claim(claim.id, tenant_id)
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(claim_id = %claim.id, error = %e, "Conflict query failed");
                    vec![]
                }
            };
            for c in &claim_conflicts {
                if !seen_conflicts.contains(&c.id) {
                    seen_conflicts.insert(c.id);
                    conflicts.push(ConflictSummary::from(c));

                    if req.delegate_to_sampling {
                        let claim_a_content = claims
                            .iter()
                            .find(|x| x.id == c.claim_a_id)
                            .map(|x| x.content.clone())
                            .unwrap_or_else(|| format!("Claim {}", c.claim_a_id));
                        let claim_b_content = claims
                            .iter()
                            .find(|x| x.id == c.claim_b_id)
                            .map(|x| x.content.clone())
                            .unwrap_or_else(|| format!("Claim {}", c.claim_b_id));

                        sampling_conflicts.push(crate::server::ConflictInfo {
                            claim_a: claim_a_content,
                            claim_b: claim_b_content,
                            conflict_type: Some(format!("{:?}", c.conflict_type)),
                        });
                    }
                }
            }
        }

        // Delegate to sampling for conflict analysis if requested
        if req.delegate_to_sampling
            && !sampling_conflicts.is_empty()
            && let Some(ref client) = llm_client
        {
            let knowledge_items: Vec<crate::server::KnowledgeItem> = claims
                .iter()
                .map(|c| crate::server::KnowledgeItem {
                    id: c.id.to_string(),
                    content: c.content.clone(),
                    confidence: c.confidence as f32,
                    source: None,
                })
                .collect();

            let context = crate::server::SamplingContext {
                knowledge_items,
                conflicts: sampling_conflicts,
                query_context: Some(req.query.clone()),
                extra: std::collections::HashMap::new(),
            };

            let sampling_req = crate::server::SamplingRequest {
                sampling_type: crate::server::SamplingType::ConflictAnalysis,
                context,
                prompt: "Analyze the conflicts detected in the knowledge base and provide insights"
                    .to_string(),
                max_tokens: 1024,
            };

            let client_ref: &dyn LlmClient = client.as_ref();
            if let Ok(sampling_result) = call_sampling_protocol(client_ref, sampling_req).await {
                for conflict in &mut conflicts {
                    conflict.sampling_analysis = Some(sampling_result.content.clone());
                }
            }
        }
    }

    // 7. Generate prediction if requested
    let mut prediction = if req.include_predictions && !merged_results.is_empty() {
        Some(generate_prediction(&req.query, &claims, &related_by_graph, llm_client.clone()).await)
    } else {
        None
    };

    // Delegate to sampling for prediction generation if requested
    if req.delegate_to_sampling
        && prediction.is_some()
        && !claims.is_empty()
        && let Some(ref client) = llm_client
    {
        let knowledge_items: Vec<crate::server::KnowledgeItem> = claims
            .iter()
            .map(|c| crate::server::KnowledgeItem {
                id: c.id.to_string(),
                content: c.content.clone(),
                confidence: c.confidence as f32,
                source: None,
            })
            .collect();

        let context = crate::server::SamplingContext {
            knowledge_items,
            conflicts: vec![],
            query_context: Some(req.query.clone()),
            extra: std::collections::HashMap::new(),
        };

        let sampling_req = crate::server::SamplingRequest {
            sampling_type: crate::server::SamplingType::PredictionGeneration,
            context,
            prompt: "Generate a more detailed prediction based on the knowledge base".to_string(),
            max_tokens: 1024,
        };

        let client_ref: &dyn LlmClient = client.as_ref();
        if let Ok(sampling_result) = call_sampling_protocol(client_ref, sampling_req).await
            && let Some(ref mut pred) = prediction
        {
            pred.sampling_analysis = Some(sampling_result.content);
        }
    }

    // 8. Detect knowledge gaps if requested (Issue #151: auto-identification)
    let knowledge_gaps = if req.include_gaps && !merged_results.is_empty() {
        detect_and_record_knowledge_gaps(
            &req.query,
            &claims,
            &related_by_graph,
            &req.context.domain,
            &conflicts,
            tenant_id,
            gap_store,
        )
        .await
    } else {
        vec![]
    };

    // 9. Determine freshness
    let newest_source = claims.iter().map(|c| c.t_known).max();
    let oldest_source = claims.iter().map(|c| c.t_known).min();
    let staleness_warning = claims.is_empty()
        || newest_source
            .map(|d| (chrono::Utc::now() - d).num_days() > 90)
            .unwrap_or(false);

    // 10. Build response
    let execution_time_ms = start_time.elapsed().as_millis() as u64;
    let related_node_count = related_by_graph.len();
    let conflict_count = conflicts.len();

    let response = McpQueryResponse {
        query_hash,
        query_context: req.query.clone(),
        best_belief,
        related_by_graph,
        conflicts,
        prediction,
        knowledge_gaps,
        freshness: FreshnessInfo {
            newest_source,
            oldest_source,
            staleness_warning,
        },
        cache_status: CacheStatus::Miss,
        cognitive_path: Some(CognitivePath::System2),
        metadata: QueryMetadata {
            execution_time_ms,
            cache_hit_rate: 0.0,
            processed_claims: claims.len(),
            related_node_count,
            conflict_count,
        },
    };

    // 11. Update cache
    let cache_entry = QueryCacheEntry::new(query_hash, response.clone());
    cache_store.set_cached(tenant_id, &cache_entry).await?;

    // 12. Buffer activation updates (flushed by background task)
    for claim in &claims {
        if let Some(buf) = activation_buffer {
            buf.push(claim.id, tenant_id, 0.1).await;
        } else {
            claim_store
                .update_activation(claim.id, tenant_id, 0.1)
                .await
                .ok();
        }
    }

    Ok(response)
}

/// Call sampling protocol for advanced LLM-based analysis
pub(crate) async fn call_sampling_protocol(
    llm_client: &dyn LlmClient,
    sampling_req: crate::server::SamplingRequest,
) -> std::result::Result<crate::server::SamplingResponse, JsonRpcError> {
    use crate::server::{SamplingResponse, SamplingType};

    let prompt = match sampling_req.sampling_type {
        SamplingType::ConflictAnalysis => {
            let context = &sampling_req.context;
            let mut prompt = format!(
                "You are an expert at analyzing conflicts between knowledge claims.\n\n\
                Please analyze the following conflicting knowledge items and provide:\n\
                1. A clear identification of the conflict\n\
                2. Possible resolutions or reconciliation\n\
                3. Recommended actions\n\n\
                Query Context: {}\n\n",
                context.query_context.as_deref().unwrap_or("N/A")
            );

            for (i, item) in context.knowledge_items.iter().enumerate() {
                prompt.push_str(&format!(
                    "\n--- Knowledge Item {} ---\nContent: {}\nConfidence: {:.2}\nSource: {}\n",
                    i + 1,
                    item.content,
                    item.confidence,
                    item.source.as_deref().unwrap_or("Unknown")
                ));
            }

            if !context.conflicts.is_empty() {
                prompt.push_str("\n--- Known Conflicts ---\n");
                for conflict in &context.conflicts {
                    prompt.push_str(&format!(
                        "Claim A: {}\nClaim B: {}\nType: {}\n",
                        conflict.claim_a,
                        conflict.claim_b,
                        conflict.conflict_type.as_deref().unwrap_or("Unknown")
                    ));
                }
            }

            prompt.push_str("\nPlease provide your analysis:");
            prompt
        }
        SamplingType::KnowledgeValidation => {
            let context = &sampling_req.context;
            let mut prompt = String::from(
                "You are an expert at validating knowledge claims.\n\n\
                Please evaluate the following knowledge item(s) for:\n\
                1. Factual accuracy\n\
                2. Source reliability\n\
                3. Confidence assessment\n\
                4. Potential issues or concerns\n\n",
            );

            for (i, item) in context.knowledge_items.iter().enumerate() {
                prompt.push_str(&format!(
                    "\n--- Knowledge Item {} ---\nContent: {}\nConfidence: {:.2}\nSource: {}\n",
                    i + 1,
                    item.content,
                    item.confidence,
                    item.source.as_deref().unwrap_or("Unknown")
                ));
            }

            if let Some(query_ctx) = &context.query_context {
                prompt.push_str(&format!("\nQuery Context: {}\n", query_ctx));
            }

            prompt.push_str("\nPlease provide your validation assessment:");
            prompt
        }
        SamplingType::PredictionGeneration => {
            let context = &sampling_req.context;
            let mut prompt = String::from(
                "You are an expert at generating informed predictions based on knowledge.\n\n\
                Based on the following knowledge items and context, please generate:\n\
                1. Likely future outcomes\n\
                2. Key factors influencing the prediction\n\
                3. Confidence level and uncertainty factors\n\
                4. Recommended monitoring points\n\n",
            );

            for (i, item) in context.knowledge_items.iter().enumerate() {
                prompt.push_str(&format!(
                    "\n--- Knowledge Item {} ---\nContent: {}\nConfidence: {:.2}\n",
                    i + 1,
                    item.content,
                    item.confidence
                ));
            }

            if let Some(query_ctx) = &context.query_context {
                prompt.push_str(&format!("\nQuery/Task: {}\n", query_ctx));
            }

            prompt.push_str("\nPlease provide your prediction:");
            prompt
        }
    };

    let llm_req = LlmRequest {
        messages: vec![Message {
            role: Role::User,
            content: prompt,
        }],
        temperature: 0.7,
        max_tokens: Some(sampling_req.max_tokens),
        ..Default::default()
    };

    let llm_response = llm_client
        .chat(llm_req)
        .await
        .map_err(|e| JsonRpcError::new(-32000, format!("LLM call failed: {}", e)))?;

    let content = llm_response.content;
    let confidence = if content.is_empty() { 0.0 } else { 0.8 };

    Ok(SamplingResponse::new(
        &format!("{:?}", sampling_req.sampling_type).to_lowercase(),
        content,
        confidence,
    ))
}

/// Generate prediction based on claims and graph relations
async fn generate_prediction(
    query: &str,
    claims: &[EpistemicClaim],
    related_graph: &[GraphRelation],
    llm_client: Option<Arc<dyn LlmClient>>,
) -> PredictionResult {
    let avg_confidence: f64 = if !claims.is_empty() {
        claims.iter().map(|c| c.confidence).sum::<f64>() / claims.len() as f64
    } else {
        0.5
    };

    if let Some(client) = llm_client {
        let llm_prediction = generate_llm_prediction(
            query,
            claims,
            related_graph,
            client.as_ref(),
            avg_confidence,
        )
        .await;

        if let Some(prediction) = llm_prediction {
            return prediction;
        }
    }

    let content = if claims.len() < 3 {
        format!(
            "⚠️ 知识库中关于'{}'的信息有限({}条)，建议收集更多数据后决策",
            query,
            claims.len()
        )
    } else if avg_confidence > 0.7 {
        format!(
            "✅ 基于{}条高置信度知识，关于'{}'的决策可信度较高",
            claims.len(),
            query
        )
    } else if !related_graph.is_empty() {
        format!("📊 发现{}条相关联知识，可作为决策参考", related_graph.len())
    } else {
        format!("💡 关于'{}'的知识已整合完毕，建议按现有信息执行", query)
    };

    PredictionResult {
        content,
        confidence: avg_confidence * 0.8,
        method: PredictionMethod::StatisticalTrend,
        based_on_claims: claims.iter().map(|c| c.id).collect(),
        sampling_analysis: None,
    }
}

/// Generate prediction using LLM
async fn generate_llm_prediction(
    query: &str,
    claims: &[EpistemicClaim],
    related_graph: &[GraphRelation],
    client: &dyn LlmClient,
    avg_confidence: f64,
) -> Option<PredictionResult> {
    let claims_context: String = claims
        .iter()
        .take(10)
        .enumerate()
        .map(|(i, c)| {
            format!(
                "{}. [置信度 {:.0}%] {}\n",
                i + 1,
                c.confidence * 100.0,
                c.content.chars().take(200).collect::<String>()
            )
        })
        .collect();

    let graph_context: String = related_graph
        .iter()
        .take(5)
        .enumerate()
        .map(|(i, r)| {
            format!(
                "{}. {}: {}\n",
                i + 1,
                r.relation_type,
                r.content.chars().take(100).collect::<String>()
            )
        })
        .collect();

    let system_prompt = r#"你是一个知识管理系统中的决策预测助手。
你的任务是基于知识库中的相关信息，对用户的查询给出：
1. 推荐建议
2. 风险评估
3. 执行建议

请用中文回复，保持简洁（不超过100字）。
如果信息不足，请明确指出不确定性和风险。"#;

    let user_prompt = format!(
        "用户查询: {}\n\n知识库中的相关信息:\n{}\n\n知识图谱关联:\n{}\n\n请给出预测建议（推荐/风险/建议）和置信度。",
        query, claims_context, graph_context
    );

    let request = LlmRequest {
        model: std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()), // verified: 2026-03-21
        messages: vec![
            Message {
                role: Role::System,
                content: system_prompt.to_string(),
            },
            Message {
                role: Role::User,
                content: user_prompt,
            },
        ],
        temperature: 0.4,
        max_tokens: Some(200),
        top_p: None,
        stop_sequences: vec![],
    };

    match client.chat(request).await {
        Ok(response) => {
            let confidence = (avg_confidence * 0.7 + 0.3).min(1.0);

            Some(PredictionResult {
                content: response.content,
                confidence,
                method: PredictionMethod::LlmBeliefContext,
                based_on_claims: claims.iter().map(|c| c.id).collect(),
                sampling_analysis: None,
            })
        }
        Err(e) => {
            tracing::warn!("LLM prediction failed: {}, falling back to rule-based", e);
            None
        }
    }
}

/// Detect knowledge gaps based on query and available claims
fn detect_knowledge_gaps(
    query: &str,
    claims: &[EpistemicClaim],
    related_graph: &[GraphRelation],
    domain: &Option<String>,
    conflicts: &[ConflictSummary],
) -> Vec<String> {
    let mut gaps = Vec::new();

    if claims.is_empty() {
        gaps.push(format!("完全缺失关于'{}'的知识储备", query));
        return gaps;
    }

    if claims.len() < 3 {
        gaps.push(format!(
            "知识稀疏：当前仅有{}条相关记录，不足以进行多方验证",
            claims.len()
        ));
    }

    let avg_confidence: f64 =
        claims.iter().map(|c| c.confidence).sum::<f64>() / claims.len() as f64;
    if avg_confidence < 0.5 {
        gaps.push(format!(
            "置信度缺口：平均置信度仅为{:.2}，建议补充权威数据源",
            avg_confidence
        ));
    }

    if related_graph.is_empty() && claims.len() > 2 {
        gaps.push("关联缺口：发现孤立知识点，知识库中缺乏逻辑连接".to_string());
    }

    let now = chrono::Utc::now();
    let newest = match claims.iter().map(|c| c.t_known).max() {
        Some(t) => t,
        None => return gaps,
    };
    let age_days = (now - newest).num_days();
    if age_days > 30 {
        gaps.push(format!(
            "时效性缺口：最新知识已是{}天前，可能已过时",
            age_days
        ));
    }

    if claims.len() >= 3 && conflicts.len() as f64 / claims.len() as f64 > 1.0 {
        gaps.push("认知冲突：该领域存在大量矛盾信息，知识一致性极低".to_string());
    }

    if let Some(d) = domain
        && claims.len() < 5
    {
        gaps.push(format!("领域缺口：'{}'领域深度不足，建议定向采集", d));
    }

    gaps
}

/// Detect and automatically record knowledge gaps to the GapStore
async fn detect_and_record_knowledge_gaps(
    query: &str,
    claims: &[EpistemicClaim],
    related_graph: &[GraphRelation],
    domain: &Option<String>,
    conflicts: &[ConflictSummary],
    tenant_id: &str,
    gap_store: &dyn GapStore,
) -> Vec<String> {
    let gaps = detect_knowledge_gaps(query, claims, related_graph, domain, conflicts);

    for gap_description in &gaps {
        let priority = if gap_description.contains("完全缺失")
            || gap_description.contains("置信度缺口")
        {
            "high"
        } else if gap_description.contains("领域缺口") || gap_description.contains("知识稀疏")
        {
            "medium"
        } else {
            "low"
        };

        let gap_record = KnowledgeGapRecord {
            gap_id: uuid::Uuid::new_v4(),
            tenant_id: tenant_id.to_string(),
            domain: domain.clone().unwrap_or_else(|| "unclassified".to_string()),
            description: gap_description.clone(),
            priority: priority.to_string(),
            status: "open".to_string(),
            reported_at: chrono::Utc::now(),
            filled_at: None,
        };

        if let Err(e) = gap_store.record_gap(&gap_record).await {
            tracing::warn!("Failed to auto-record knowledge gap: {}", e);
        }
    }

    gaps
}

/// Strip YAML frontmatter (---\n...\n---) from content
fn strip_yaml_frontmatter(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.starts_with("---") {
        if let Some(end_pos) = trimmed[3..].find("\n---") {
            let after = &trimmed[3 + end_pos + 4..];
            return after.trim_start_matches('\n').to_string();
        }
    }
    content.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_entities_from_person_query() {
        let entities = extract_query_entities("What did Caroline research?");
        assert_eq!(entities, vec!["Caroline"]);
    }

    #[test]
    fn test_extract_entities_multiple() {
        let entities = extract_query_entities("Did Caroline and Melanie go to Paris together?");
        assert_eq!(entities, vec!["Caroline", "Melanie", "Paris"]);
    }

    #[test]
    fn test_extract_entities_skips_question_words() {
        let entities = extract_query_entities("What is the API rate limit?");
        assert!(entities.is_empty(), "got: {:?}", entities);
    }

    #[test]
    fn test_extract_entities_skips_pronouns() {
        let entities = extract_query_entities("Where did He go after the meeting?");
        assert!(entities.is_empty(), "got: {:?}", entities);
    }

    #[test]
    fn test_extract_entities_handles_punctuation() {
        let entities = extract_query_entities("When was Dr. Sarah Chen hired?");
        assert!(entities.contains(&"Sarah".to_string()));
        assert!(entities.contains(&"Chen".to_string()));
    }

    #[test]
    fn test_extract_entities_empty_query() {
        let entities = extract_query_entities("");
        assert!(entities.is_empty());
    }

    #[test]
    fn test_extract_entities_no_caps() {
        let entities = extract_query_entities("what is the weather today?");
        assert!(entities.is_empty());
    }

    #[test]
    fn test_extract_entities_location() {
        let entities = extract_query_entities("Who founded the company in San Francisco?");
        assert!(entities.contains(&"San".to_string()));
        assert!(entities.contains(&"Francisco".to_string()));
    }

    #[test]
    fn test_granularity_detection() {
        assert_eq!(
            detect_query_granularity("When did Caroline arrive?"),
            QueryGranularity::Precise
        );
        assert_eq!(
            detect_query_granularity("Describe the company culture"),
            QueryGranularity::General
        );
        assert_eq!(
            detect_query_granularity("What did Caroline research?"),
            QueryGranularity::Thematic
        );
    }
}
