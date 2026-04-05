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

    // LLM retrieval feature gate — controls query rewriting and candidate re-ranking
    let llm_retrieval_enabled = std::env::var("LLM_RETRIEVAL_ENABLED")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(true); // enabled by default when llm_client is available

    let llm_rewrite_enabled = llm_retrieval_enabled
        && std::env::var("LLM_REWRITE_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false); // disabled by default — high latency, low accuracy

    let reranker_enabled = std::env::var("RERANKER_ENABLED")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(true); // cross-encoder reranker enabled by default

    // LLM Query Rewriting — expand query with related terms for better recall
    let effective_query = if llm_rewrite_enabled {
        if let Some(ref llm) = llm_client {
            match rewrite_query_with_llm(llm.as_ref(), &req.query).await {
                Ok(expanded) => {
                    tracing::debug!(original = %req.query, expanded = %expanded, "LLM query rewrite");
                    expanded
                }
                Err(e) => {
                    tracing::debug!(error = %e, "LLM query rewrite failed, using original");
                    req.query.clone()
                }
            }
        } else {
            req.query.clone()
        }
    } else {
        req.query.clone()
    };

    // Question-to-statement normalization: strip question syntax for better vector matching.
    // Root cause: BGE-M3 embedding space has higher similarity for question↔question pairs
    // than for question↔statement pairs. "What did Caroline research?" is more similar to
    // "Melanie: What kinda jobs?" (another question) than to "Caroline: Researching adoption
    // agencies" (the actual answer, a statement). By stripping question words, we convert
    // the query into keyword form that better matches statement-form answers in the DB.
    let search_query = normalize_question_to_keywords(&effective_query);

    // Default to semantic layer — prevents working/episodic memory from leaking
    // into queries that don't explicitly request them.
    // Agents share the semantic layer within the same tenant (shared brain).
    // Working/episodic require explicit memory_layer + session_id to access.
    let effective_layer = req.memory_layer.as_deref().or(Some("semantic"));

    // === TWO-STAGE RETRIEVAL (cognitive architecture: coarse filter -> fine rank) ===
    //
    // Stage 1: If query mentions a speaker name + topic words, use targeted
    //          full-text search scoped to that speaker (high recall, low cost).
    // Stage 2: If Stage 1 yields >= 3 candidates, skip the global vector+BM25
    //          search entirely — candidates are already speaker-filtered.
    //          Otherwise fall back to the original global search pipeline.

    let stage1_min_candidates: usize = std::env::var("STAGE1_MIN_CANDIDATES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let stage1_recall_limit: usize = std::env::var("STAGE1_RECALL_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50);

    // Stop-words to exclude when extracting topic words from the query
    const STOP_WORDS: &[&str] = &[
        "what", "when", "where", "which", "who", "whom", "whose", "how", "does", "did", "have",
        "been", "would", "could", "should", "about", "their", "they", "them", "this", "that",
        "from", "with", "into", "the", "and", "for", "are", "was", "were", "will", "has", "had",
        "not", "but", "can", "any", "all", "some", "each", "than", "then", "just", "also", "more",
    ];

    let query_lower = req.query.to_lowercase();

    // Detect speaker names from query — any capitalized word that appears as
    // "Name:" prefix in stored claims. No hardcoded list — works for any speaker.
    let detected_speakers: Vec<String> = req
        .query
        .split_whitespace()
        .filter(|w| {
            let trimmed = w.trim_matches(|c: char| !c.is_alphanumeric());
            trimmed.len() > 1
                && trimmed.chars().next().map_or(false, |c| c.is_uppercase())
                && trimmed.chars().skip(1).all(|c| c.is_lowercase())
                && !STOP_WORDS.contains(&trimmed.to_lowercase().as_str())
        })
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .collect();

    let topic_words: Vec<&str> = query_lower
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .filter(|w| !STOP_WORDS.contains(w))
        .filter(|w| !detected_speakers.iter().any(|s| s == *w))
        .collect();

    let mut _two_stage_used = false;

    // Attempt Stage 1: speaker-scoped full-text search
    let stage1_candidates: Option<Vec<EpistemicClaim>> = if !detected_speakers.is_empty()
        && !topic_words.is_empty()
    {
        let speaker = &detected_speakers[0];
        let search_query = format!("{} {}", speaker, topic_words.join(" "));

        let raw_results = claim_store
            .search_claims(tenant_id, &search_query, stage1_recall_limit)
            .await
            .unwrap_or_default();

        // Filter to claims spoken BY this speaker (starts_with check)
        let speaker_lower = speaker.to_lowercase();
        let filtered: Vec<EpistemicClaim> = raw_results
            .into_iter()
            .filter(|c| {
                let content_lower = c.content.to_lowercase();
                content_lower.starts_with(&format!("{}:", speaker_lower))
                    || content_lower.starts_with(&format!("{} :", speaker_lower))
            })
            .collect();

        if filtered.len() >= stage1_min_candidates {
            tracing::debug!(
                speaker = speaker,
                candidates = filtered.len(),
                topic = topic_words.join(" "),
                "Two-stage retrieval: Stage 1 speaker-filtered candidates sufficient"
            );
            Some(filtered)
        } else {
            tracing::debug!(
                speaker = speaker,
                candidates = filtered.len(),
                min_required = stage1_min_candidates,
                "Two-stage retrieval: insufficient Stage 1 candidates, falling back to global search"
            );
            None
        }
    } else {
        None
    };

    // Branch: Stage 1 success → use candidates directly; else → global search
    let (mut claims, mut claim_ids, vector_matches) = if let Some(candidates) = stage1_candidates {
        _two_stage_used = true;
        let ids: Vec<uuid::Uuid> = candidates.iter().map(|c| c.id).collect();
        // No vector_matches when using two-stage path (graph diffusion still runs below)
        (candidates, ids, Vec::new())
    } else {
        // --- Original global search pipeline (vector + BM25 + RRF) ---

        // 2. Vector search - embed query using embedding service (uses LLM-expanded query)
        let query_vector = if let Some(ref client) = embedding_client {
            let embedding_service = EmbeddingService::new(client.clone());
            match embedding_service.embed(&search_query).await {
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

        let vector_matches = vector_store
            .search(
                query_vector,
                tenant_id,
                req.context.max_results,
                effective_layer,
            )
            .await?;

        // 2a. Similarity threshold gate — filter out irrelevant nearest neighbors
        // CJK queries need a lower threshold: cross-lingual embedding similarity is lower
        let is_cjk = req.query.chars().any(|c| c > '\u{2E80}');
        let default_threshold = if is_cjk { 0.3 } else { 0.5 };
        let min_similarity: f64 = std::env::var("MIN_SIMILARITY_THRESHOLD")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default_threshold);

        let vector_matches: Vec<_> = vector_matches
            .into_iter()
            .filter(|m| m.score >= min_similarity)
            .collect();

        // 2b. Full-text keyword search — catches exact term matches that vectors miss
        //     Uses expanded query for broader recall
        let text_matches = claim_store
            .search_claims(tenant_id, &search_query, req.context.max_results as usize)
            .await
            .unwrap_or_default();

        // 2c. Merge via RRF (Reciprocal Rank Fusion) — k=60 is the standard constant
        let rrf_k = 60.0_f64;
        let mut rrf_scores: std::collections::HashMap<uuid::Uuid, f64> =
            std::collections::HashMap::new();

        for (rank, m) in vector_matches.iter().enumerate() {
            *rrf_scores.entry(m.id).or_default() += 1.0 / (rrf_k + rank as f64);
        }
        for (rank, c) in text_matches.iter().enumerate() {
            *rrf_scores.entry(c.id).or_default() += 1.0 / (rrf_k + rank as f64);
        }

        // Build a quick lookup of text-matched claims to avoid redundant DB round-trips
        let text_claim_map: std::collections::HashMap<uuid::Uuid, &EpistemicClaim> =
            text_matches.iter().map(|c| (c.id, c)).collect();

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

        (claims, claim_ids, vector_matches)
    };

    // Track seen IDs for graph diffusion deduplication
    let mut seen_ids: std::collections::HashSet<uuid::Uuid> = claim_ids.iter().copied().collect();

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

    // 3e. Speaker-aware prioritization (Tulving encoding specificity)
    // When the query mentions a person's name, prioritize claims spoken BY that person.
    // "What did Caroline research?" should prefer "Caroline: ..." over "Melanie: Wow, Caroline!"
    {
        let query_lower = req.query.to_lowercase();
        let speakers: Vec<String> = req
            .query
            .split_whitespace()
            .filter(|w| {
                let t = w.trim_matches(|c: char| !c.is_alphanumeric());
                t.len() > 1
                    && t.chars().next().map_or(false, |c| c.is_uppercase())
                    && t.chars().skip(1).all(|c| c.is_lowercase())
            })
            .map(|w| {
                w.trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase()
            })
            .collect();

        if !speakers.is_empty() && claims.len() > 1 {
            // Stable sort: claims where the speaker matches sort first
            claims.sort_by(|a, b| {
                let a_content = a.content.to_lowercase();
                let b_content = b.content.to_lowercase();
                let a_speaker_match = speakers.iter().any(|s| {
                    // Check if the claim starts with "Speaker: ..." (is spoken BY that person)
                    a_content.starts_with(&format!("{}: ", s))
                        || a_content.starts_with(&format!("{}:", s))
                });
                let b_speaker_match = speakers.iter().any(|s| {
                    b_content.starts_with(&format!("{}: ", s))
                        || b_content.starts_with(&format!("{}:", s))
                });
                b_speaker_match.cmp(&a_speaker_match) // speaker-match first
            });
            claim_ids = claims.iter().map(|c| c.id).collect();
        }
    }

    // 3f. Cross-encoder reranking (local BGE-Reranker-v2-m3 via TEI, ~25ms)
    if reranker_enabled && claims.len() > 1 {
        match rerank_with_cross_encoder(&req.query, &claims).await {
            Ok(reranked) => {
                tracing::debug!(
                    original_top = %claims.first().map(|c| c.id.to_string()).unwrap_or_default(),
                    reranked_top = %reranked.first().map(|c| c.id.to_string()).unwrap_or_default(),
                    "Cross-encoder rerank applied"
                );
                claims = reranked;
                claim_ids = claims.iter().map(|c| c.id).collect();
            }
            Err(e) => {
                tracing::debug!(error = %e, "Cross-encoder rerank failed, using original order");
            }
        }
    }

    // 3g. Record rehearsal for retrieved claims (S3: read equals write)
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

    let (merged_results, related_by_graph) = merge_results(
        &vector_matches,
        &all_graph_nodes,
        &claim_tuples,
        &merge_config,
    );

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

/// Rewrite a user query with LLM to expand search terms for better recall.
/// Combines original query with LLM-generated synonyms and related terms.
async fn rewrite_query_with_llm(llm: &dyn LlmClient, query: &str) -> Result<String> {
    let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()); // verified: 2026-03-21
    let request = LlmRequest {
        model,
        messages: vec![Message {
            role: Role::User,
            content: format!(
                "Rewrite this question as a search query. Add synonyms and related terms that would appear in the answer. Output ONLY the expanded search terms, no explanation.\n\nQuestion: {}\n\nSearch terms:",
                query
            ),
        }],
        temperature: 0.0,
        max_tokens: Some(500), // MiniMax needs ~300 tokens for <think> + actual output
        ..Default::default()
    };

    let response = llm
        .chat(request)
        .await
        .map_err(|e| cogkos_core::CogKosError::ExternalError(format!("LLM: {}", e)))?;

    // Strip <think>...</think> tags (MiniMax M2.5/M2.7 reasoning output)
    let cleaned = strip_think_tags(&response.content);

    // Combine original query with LLM expansion for maximum recall
    let expanded = if cleaned.is_empty() {
        query.to_string() // fallback if LLM only output thinking
    } else {
        format!("{} {}", query, cleaned)
    };
    Ok(expanded)
}

/// Rerank candidates using local BGE-Reranker-v2-m3 (TEI endpoint).
/// ~25ms latency vs ~3-8s for LLM reranking.
async fn rerank_with_cross_encoder(
    query: &str,
    candidates: &[EpistemicClaim],
) -> Result<Vec<EpistemicClaim>> {
    let reranker_url = std::env::var("RERANKER_URL")
        .unwrap_or_else(|_| "http://localhost:8091/rerank".to_string()); // verified: 2026-03-30

    let num_candidates = candidates.len().min(20);
    let texts: Vec<String> = candidates
        .iter()
        .take(num_candidates)
        .map(|c| {
            if c.content.len() > 512 {
                c.content[..c.content.floor_char_boundary(512)].to_string()
            } else {
                c.content.clone()
            }
        })
        .collect();

    let body = serde_json::json!({
        "query": query,
        "texts": texts,
    });

    let client = reqwest::Client::new();
    let response = client
        .post(&reranker_url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| cogkos_core::CogKosError::ExternalError(format!("Reranker: {}", e)))?;

    let results = response
        .json::<Vec<serde_json::Value>>()
        .await
        .map_err(|e| cogkos_core::CogKosError::ExternalError(format!("Reranker parse: {}", e)))?;

    // Sort by score descending
    let mut scored: Vec<(usize, f64)> = results
        .iter()
        .filter_map(|r| {
            let idx = r.get("index")?.as_u64()? as usize;
            let score = r.get("score")?.as_f64()?;
            Some((idx, score))
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Rebuild claims in reranked order
    let mut reranked = Vec::with_capacity(candidates.len());
    let mut seen = std::collections::HashSet::new();
    for (idx, _score) in &scored {
        if *idx < candidates.len() && seen.insert(*idx) {
            reranked.push(candidates[*idx].clone());
        }
    }
    // Append any unranked candidates (beyond num_candidates limit)
    for (i, c) in candidates.iter().enumerate() {
        if !seen.contains(&i) {
            reranked.push(c.clone());
        }
    }

    Ok(reranked)
}

/// Normalize question-form queries into keyword-form for better vector matching.
///
/// BGE-M3 embedding space has higher cosine similarity for question↔question pairs
/// than for question↔statement pairs. This causes questions like "What did Caroline
/// research?" to match other questions (e.g., "What kinda jobs?") rather than the
/// actual statement answer ("Researching adoption agencies").
///
/// By stripping question syntax, we convert to keyword form that better matches
/// statement-form content stored in the database.
fn normalize_question_to_keywords(query: &str) -> String {
    let q = query.trim();

    // Remove trailing question mark
    let q = q.strip_suffix('?').unwrap_or(q);

    // Remove leading question words/phrases
    let question_prefixes = [
        "what did ",
        "what does ",
        "what is ",
        "what are ",
        "what was ",
        "what were ",
        "what has ",
        "what have ",
        "what do ",
        "what would ",
        "what could ",
        "when did ",
        "when does ",
        "when is ",
        "when was ",
        "when were ",
        "where did ",
        "where does ",
        "where is ",
        "where was ",
        "who did ",
        "who does ",
        "who is ",
        "who was ",
        "who are ",
        "which ",
        "how did ",
        "how does ",
        "how is ",
        "how long ",
        "how many ",
        "how much ",
        "how often ",
        "did ",
        "does ",
        "is ",
        "are ",
        "was ",
        "were ",
        "has ",
        "have ",
        "would ",
        "could ",
        "can ",
        "do ",
    ];

    let lower = q.to_lowercase();
    let mut result = q.to_string();

    for prefix in &question_prefixes {
        if lower.starts_with(prefix) {
            result = q[prefix.len()..].to_string();
            break;
        }
    }

    // Remove filler words that don't help search
    let fillers = ["likely ", "probably ", "actually ", "really ", "still "];
    for filler in &fillers {
        result = result.replace(filler, "");
    }

    let trimmed = result.trim().to_string();
    if trimmed.is_empty() {
        query.to_string() // fallback to original if normalization removes everything
    } else {
        trimmed
    }
}

/// Strip `<think>...</think>` tags from LLM responses (MiniMax M2.5/M2.7 reasoning output).
fn strip_think_tags(content: &str) -> String {
    let mut result = content.to_string();
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result.find("</think>") {
            result = format!("{}{}", &result[..start], &result[end + 8..]);
        } else {
            // Unclosed <think> tag — strip from <think> to end
            result = result[..start].to_string();
            break;
        }
    }
    result.trim().to_string()
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
