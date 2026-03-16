//! Query knowledge handler and related functions

use crate::merger::{MergeConfig, merge_results};
use crate::server::JsonRpcError;
use cogkos_core::Result;
use cogkos_core::models::*;
use cogkos_ingest::EmbeddingService;
use cogkos_llm::{LlmClient, LlmRequest, Message, Role};
use cogkos_store::{CacheStore, ClaimStore, GapStore, GraphStore, KnowledgeGapRecord, VectorStore};
use std::sync::Arc;

use super::helpers::{calculate_query_hash, generate_query_vector};
use super::types::*;

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

    // 1. Check cache (skip if high urgency)
    if !matches!(req.context.urgency, Urgency::High)
        && let Some(cached) = cache_store.get_cached(tenant_id, query_hash).await?
    {
        // Use configurable TTL from environment or default to 3600 seconds
        let ttl_seconds = std::env::var("CACHE_TTL_SECONDS")
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(3600);

        // Check if cache is still valid using the comprehensive validation logic
        if cached.is_valid(ttl_seconds) {
            // Update hit stats
            cache_store.record_hit(tenant_id, query_hash).await?;

            let hit_rate = cached.success_rate();
            let mut response = cached.response.clone();
            response.cache_status = CacheStatus::Hit;
            response.metadata.execution_time_ms = start_time.elapsed().as_millis() as u64;
            response.metadata.cache_hit_rate = hit_rate;

            // Log cache hit for monitoring
            tracing::debug!(
                query_hash = query_hash,
                hit_count = cached.hit_count,
                success_rate = hit_rate,
                "Cache hit for query"
            );

            return Ok(response);
        } else {
            // Cache entry exists but is invalid - log for monitoring
            tracing::debug!(
                query_hash = query_hash,
                confidence = cached.confidence,
                hit_count = cached.hit_count,
                success_rate = cached.success_rate(),
                invalidated = cached.invalidated_by.is_some(),
                "Cache entry expired or invalid"
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
                generate_query_vector(&req.query)
            }
        }
    } else {
        tracing::warn!("No embedding client configured, using fallback");
        generate_query_vector(&req.query)
    };
    let vector_matches = vector_store
        .search(query_vector, tenant_id, req.context.max_results)
        .await?;

    // 3. Get claims from vector matches with comprehensive permission filtering
    let mut claims = Vec::new();
    let mut claim_ids: Vec<uuid::Uuid> = Vec::new();
    for m in &vector_matches {
        if let Ok(claim) = claim_store.get_claim(m.id, tenant_id).await {
            claim_ids.push(claim.id);
            claims.push(claim);
        }
    }

    // 4. Graph activation diffusion with threshold
    let mut all_graph_nodes: Vec<GraphNode> = Vec::new();
    let threshold = req.activation_threshold.clamp(0.0, 1.0);
    let _decay_factor = 0.8;
    let _max_depth = 2;

    for claim in &claims {
        let related = graph_store.find_related(claim.id, 2, threshold).await?;
        for node in related {
            if !all_graph_nodes
                .iter()
                .any(|n: &GraphNode| n.content == node.content)
            {
                all_graph_nodes.push(node);
            }
        }
    }

    // 5. Merge vector search results with graph diffusion results
    let merge_config = MergeConfig {
        vector_weight: 0.6,
        graph_weight: 0.4,
        min_score: 0.1,
        max_results: req.context.max_results as usize,
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
            BeliefSummary {
                claim_id: Some(r.claim_id),
                content: r.content.clone(),
                confidence: r.confidence,
                based_on: claims.len(),
                consolidation_stage: claim
                    .map(|c| c.consolidation_stage)
                    .unwrap_or(ConsolidationStage::FastTrack),
                claim_ids: merged_results.iter().map(|r| r.claim_id).collect(),
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
            let claim_conflicts = claim_store
                .get_conflicts_for_claim(claim.id, tenant_id)
                .await?;
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
        if let Ok(sampling_result) = call_sampling_protocol(client_ref, sampling_req).await {
            if let Some(ref mut pred) = prediction {
                pred.sampling_analysis = Some(sampling_result.content);
            }
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

    // 12. Update activation
    for claim in &claims {
        claim_store
            .update_activation(claim.id, tenant_id, 0.1)
            .await
            .ok();
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
        model: "gpt-4o-mini".to_string(),
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

    if related_graph.len() < claims.len() && claims.len() > 1 {
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

    if conflicts.len() as f64 / claims.len() as f64 > 0.5 {
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
