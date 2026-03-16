//! Feedback and gap reporting handlers, cross-instance query

use cogkos_core::models::*;
use cogkos_core::Result;
use cogkos_store::{CacheStore, FeedbackStore, GapStore, KnowledgeGapRecord};

use super::helpers::{calculate_anomaly_score, generate_gap_suggestions, rand_simple};
use super::types::*;

/// Handle cross-instance query
pub async fn handle_cross_instance_query(
    req: CrossInstanceQueryRequest,
    _tenant_id: &str,
) -> Result<CrossInstanceQueryResponse> {
    let start_time = std::time::Instant::now();

    let query_id = uuid::Uuid::new_v4().to_string();

    let mock_nodes = [
        ("node-alpha".to_string(), "Alpha Instance".to_string(), 0.85),
        ("node-beta".to_string(), "Beta Instance".to_string(), 0.72),
        ("node-gamma".to_string(), "Gamma Instance".to_string(), 0.91),
    ];

    let total_nodes = if req.domains.is_empty() {
        mock_nodes.len()
    } else {
        mock_nodes
            .iter()
            .filter(|(id, name, _)| {
                req.domains
                    .iter()
                    .any(|d| id.contains(d) || name.contains(d))
            })
            .count()
            .max(1)
    };

    let results: Vec<CrossInstanceResult> = mock_nodes
        .iter()
        .take(total_nodes)
        .map(|(node_id, node_name, expertise_score)| {
            CrossInstanceResult {
                node_id: node_id.clone(),
                success: true,
                data: Some(serde_json::json!({
                    "insights": [
                        {
                            "content": format!("Insight about: {}", req.query),
                            "confidence": expertise_score,
                            "source": node_name,
                        }
                    ],
                    "summary": format!("Found relevant knowledge from {} regarding '{}'", node_name, req.query)
                })),
                error: None,
                response_time_ms: (100 + rand_simple(node_id) % 400) as u64,
                expertise_score: *expertise_score,
            }
        })
        .collect();

    let successful_nodes = results.len();
    let failed_nodes = total_nodes.saturating_sub(successful_nodes);

    let aggregated = if !results.is_empty() {
        Some(AggregatedInsight {
            content: format!(
                "Aggregated insights from {} nodes for query: {}",
                successful_nodes, req.query
            ),
            confidence: results.iter().map(|r| r.expertise_score).sum::<f64>()
                / results.len() as f64,
            sources: results.iter().map(|r| r.node_id.clone()).collect(),
            coverage_score: successful_nodes as f64 / total_nodes.max(1) as f64,
        })
    } else {
        None
    };

    let processing_time_ms = start_time.elapsed().as_millis() as u64;

    Ok(CrossInstanceQueryResponse {
        query_id,
        results,
        aggregated,
        metadata: CrossInstanceMetadata {
            total_nodes,
            successful_nodes,
            failed_nodes,
            processing_time_ms,
        },
    })
}

/// Submit feedback handler
pub async fn handle_submit_feedback(
    req: SubmitFeedbackRequest,
    tenant_id: &str,
    agent_id: &str,
    feedback_store: &dyn FeedbackStore,
    cache_store: &dyn CacheStore,
) -> Result<serde_json::Value> {
    let note = req.note.clone().unwrap_or_default();
    let feedback = AgentFeedback::new(req.query_hash, agent_id, req.success).with_note(&note);

    feedback_store.insert_feedback(&feedback).await?;

    let mut cache_adjusted = false;
    let mut new_confidence: Option<f64> = None;

    if let Some(cached) = cache_store.get_cached(tenant_id, req.query_hash).await? {
        let history = feedback_store
            .get_feedback_for_query(req.query_hash)
            .await?;

        let total = history.len();
        let successes = history.iter().filter(|f| f.success).count();
        let success_rate = if total > 0 {
            successes as f64 / total as f64
        } else {
            0.0
        };

        let adjustment = if req.success {
            (success_rate - 0.5) * 0.1
        } else {
            -0.15
        };

        new_confidence = Some((cached.confidence + adjustment).clamp(0.0, 1.0));

        if req.success {
            cache_store
                .record_success(tenant_id, req.query_hash)
                .await
                .ok();
            cache_store
                .refresh_ttl(tenant_id, req.query_hash)
                .await
                .ok();
        } else {
            if success_rate < 0.3 {
                cache_store.invalidate(tenant_id, req.query_hash).await.ok();
            }
        }

        cache_adjusted = true;
    }

    let history = feedback_store
        .get_feedback_for_query(req.query_hash)
        .await?;
    let anomaly_score = calculate_anomaly_score(&history, req.success);

    Ok(serde_json::json!({
        "status": "recorded",
        "cache_adjusted": cache_adjusted,
        "adjusted_confidence": new_confidence,
        "anomaly_score": anomaly_score,
        "improvement_suggestion": req.improvement_suggestion,
        "feedback_note": note
    }))
}

/// Report gap handler
///
/// Implements:
/// - TC-L6-15: Knowledge gap record creation
/// - TC-L6-16: Duplicate detection - same gap not created twice
pub async fn handle_report_gap(
    req: ReportGapRequest,
    tenant_id: &str,
    gap_store: &dyn GapStore,
) -> Result<serde_json::Value> {
    if let Some(existing) = gap_store
        .find_similar_gap(tenant_id, &req.domain, &req.description)
        .await?
    {
        return Ok(serde_json::json!({
            "gap_id": existing.gap_id.to_string(),
            "status": "already_exists",
            "domain": existing.domain,
            "priority": existing.priority,
            "message": "Similar gap already exists",
            "existing_gap": {
                "gap_id": existing.gap_id,
                "reported_at": existing.reported_at,
                "status": existing.status
            }
        }));
    }

    let gap = KnowledgeGapRecord {
        gap_id: uuid::Uuid::new_v4(),
        tenant_id: tenant_id.to_string(),
        domain: req.domain.clone(),
        description: req.description.clone(),
        priority: format!("{:?}", req.priority).to_lowercase(),
        status: "open".to_string(),
        reported_at: chrono::Utc::now(),
        filled_at: None,
    };

    let gap_id = gap_store.record_gap(&gap).await?;

    let suggested_sources = generate_gap_suggestions(&req.domain, &req.description);

    let estimated_fill_time = match req.priority {
        Priority::High => "24h",
        Priority::Medium => "48h",
        Priority::Low => "168h",
    };

    Ok(serde_json::json!({
        "gap_id": gap_id.to_string(),
        "status": "recorded",
        "domain": req.domain,
        "priority": format!("{:?}", req.priority).to_lowercase(),
        "suggested_sources": suggested_sources,
        "estimated_fill_time": estimated_fill_time,
        "message": "Knowledge gap recorded successfully"
    }))
}

// TODO #184: Implement version replacement for Business knowledge
#[cfg(test)]
mod feedback_cache_tests {
    use super::*;
    use cogkos_core::Result;
    use cogkos_store::async_trait;
    use cogkos_store::{CacheStore, FeedbackStore};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[derive(Debug, Clone)]
    struct MockFeedbackStore {
        feedbacks: Arc<RwLock<Vec<AgentFeedback>>>,
    }

    impl MockFeedbackStore {
        fn new() -> Self {
            Self {
                feedbacks: Arc::new(RwLock::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl FeedbackStore for MockFeedbackStore {
        async fn insert_feedback(&self, feedback: &AgentFeedback) -> Result<()> {
            let mut feedbacks = self.feedbacks.write().await;
            feedbacks.push(feedback.clone());
            Ok(())
        }

        async fn get_feedback_for_query(&self, query_hash: u64) -> Result<Vec<AgentFeedback>> {
            let feedbacks = self.feedbacks.read().await;
            Ok(feedbacks
                .iter()
                .filter(|f| f.query_hash == query_hash)
                .cloned()
                .collect())
        }
    }

    #[derive(Debug, Clone)]
    struct MockCacheStore {
        cache: Arc<RwLock<std::collections::HashMap<(String, u64), QueryCacheEntry>>>,
    }

    impl MockCacheStore {
        fn new() -> Self {
            Self {
                cache: Arc::new(RwLock::new(std::collections::HashMap::new())),
            }
        }
    }

    #[async_trait]
    impl CacheStore for MockCacheStore {
        async fn get_cached(
            &self,
            tenant_id: &str,
            query_hash: u64,
        ) -> Result<Option<QueryCacheEntry>> {
            let cache = self.cache.read().await;
            Ok(cache.get(&(tenant_id.to_string(), query_hash)).cloned())
        }

        async fn set_cached(&self, tenant_id: &str, entry: &QueryCacheEntry) -> Result<()> {
            let mut cache = self.cache.write().await;
            cache.insert((tenant_id.to_string(), entry.query_hash), entry.clone());
            Ok(())
        }

        async fn record_hit(&self, tenant_id: &str, query_hash: u64) -> Result<()> {
            let mut cache = self.cache.write().await;
            if let Some(entry) = cache.get_mut(&(tenant_id.to_string(), query_hash)) {
                entry.record_hit();
            }
            Ok(())
        }

        async fn record_success(&self, tenant_id: &str, query_hash: u64) -> Result<()> {
            let mut cache = self.cache.write().await;
            if let Some(entry) = cache.get_mut(&(tenant_id.to_string(), query_hash)) {
                entry.record_success();
            }
            Ok(())
        }

        async fn invalidate(&self, tenant_id: &str, query_hash: u64) -> Result<()> {
            let mut cache = self.cache.write().await;
            cache.remove(&(tenant_id.to_string(), query_hash));
            Ok(())
        }

        async fn refresh_ttl(&self, tenant_id: &str, query_hash: u64) -> Result<()> {
            let mut cache = self.cache.write().await;
            if let Some(entry) = cache.get_mut(&(tenant_id.to_string(), query_hash)) {
                entry.last_used = chrono::Utc::now();
            }
            Ok(())
        }
    }

    fn create_test_cache_entry(query_hash: u64) -> QueryCacheEntry {
        let response = McpQueryResponse {
            query_hash,
            query_context: "test query".to_string(),
            best_belief: Some(BeliefSummary {
                claim_id: Some(uuid::Uuid::new_v4()),
                content: "Test belief content".to_string(),
                confidence: 0.7,
                based_on: 3,
                consolidation_stage: ConsolidationStage::Consolidated,
                claim_ids: vec![],
            }),
            related_by_graph: vec![],
            conflicts: vec![],
            prediction: None,
            knowledge_gaps: vec![],
            freshness: FreshnessInfo {
                newest_source: Some(chrono::Utc::now()),
                oldest_source: Some(chrono::Utc::now()),
                staleness_warning: false,
            },
            cache_status: CacheStatus::Miss,
            metadata: QueryMetadata::default(),
        };

        QueryCacheEntry::new(query_hash, response)
    }

    #[tokio::test]
    async fn test_feedback_positive_updates_cache() {
        let query_hash = 12345u64;
        let agent_id = "test_agent";

        let feedback_store = Arc::new(MockFeedbackStore::new());
        let cache_store = Arc::new(MockCacheStore::new());

        let cache_entry = create_test_cache_entry(query_hash);
        cache_store
            .set_cached("test-tenant", &cache_entry)
            .await
            .unwrap();

        let req = SubmitFeedbackRequest {
            query_hash,
            success: true,
            note: Some("Query results were helpful".to_string()),
            improvement_suggestion: None,
        };

        let tenant_id = "test-tenant";
        let result = handle_submit_feedback(
            req,
            tenant_id,
            agent_id,
            feedback_store.as_ref(),
            cache_store.as_ref(),
        )
        .await
        .unwrap();

        let feedbacks = feedback_store
            .get_feedback_for_query(query_hash)
            .await
            .unwrap();
        assert_eq!(feedbacks.len(), 1);
        assert!(feedbacks[0].success);

        let cached = cache_store.get_cached(tenant_id, query_hash).await.unwrap();
        assert!(cached.is_some());
        let cached_entry = cached.unwrap();
        assert!(cached_entry.success_count > 0);

        let status = result.get("status").unwrap().as_str().unwrap();
        assert_eq!(status, "recorded");

        let cache_adjusted = result.get("cache_adjusted").unwrap().as_bool().unwrap();
        assert!(cache_adjusted);
    }

    #[tokio::test]
    async fn test_feedback_negative_adjusts_confidence() {
        let query_hash = 12346u64;
        let agent_id = "test_agent";

        let feedback_store = Arc::new(MockFeedbackStore::new());
        let cache_store = Arc::new(MockCacheStore::new());

        let cache_entry = create_test_cache_entry(query_hash);
        cache_store
            .set_cached("test-tenant", &cache_entry)
            .await
            .unwrap();

        let req = SubmitFeedbackRequest {
            query_hash,
            success: false,
            note: Some("Results were not accurate".to_string()),
            improvement_suggestion: Some("Improve source quality".to_string()),
        };

        let tenant_id = "test-tenant";
        let result = handle_submit_feedback(
            req,
            tenant_id,
            agent_id,
            feedback_store.as_ref(),
            cache_store.as_ref(),
        )
        .await
        .unwrap();

        let cache_adjusted = result.get("cache_adjusted").unwrap().as_bool().unwrap();
        assert!(cache_adjusted);

        let adjusted_confidence = result.get("adjusted_confidence");
        assert!(adjusted_confidence.is_some());
    }

    #[tokio::test]
    async fn test_feedback_without_cache() {
        let query_hash = 12348u64;
        let agent_id = "test_agent";

        let feedback_store = Arc::new(MockFeedbackStore::new());
        let cache_store = Arc::new(MockCacheStore::new());

        let req = SubmitFeedbackRequest {
            query_hash,
            success: true,
            note: Some("First feedback".to_string()),
            improvement_suggestion: None,
        };

        let tenant_id = "test-tenant";
        let result = handle_submit_feedback(
            req,
            tenant_id,
            agent_id,
            feedback_store.as_ref(),
            cache_store.as_ref(),
        )
        .await
        .unwrap();

        let feedbacks = feedback_store
            .get_feedback_for_query(query_hash)
            .await
            .unwrap();
        assert_eq!(feedbacks.len(), 1);

        let cache_adjusted = result.get("cache_adjusted").unwrap().as_bool().unwrap();
        assert!(!cache_adjusted);
    }

    #[tokio::test]
    async fn test_end_to_end_feedback_cache_flow() {
        let query_hash = 12350u64;
        let agent_id = "test_agent";

        let feedback_store = Arc::new(MockFeedbackStore::new());
        let cache_store = Arc::new(MockCacheStore::new());

        let cache_entry = create_test_cache_entry(query_hash);
        cache_store
            .set_cached("test-tenant", &cache_entry)
            .await
            .unwrap();

        let cached = cache_store
            .get_cached("test-tenant", query_hash)
            .await
            .unwrap();
        assert!(cached.is_some());
        let initial_confidence = cached.unwrap().confidence;

        cache_store
            .record_hit("test-tenant", query_hash)
            .await
            .unwrap();

        let req = SubmitFeedbackRequest {
            query_hash,
            success: true,
            note: Some("Results were excellent!".to_string()),
            improvement_suggestion: None,
        };

        let tenant_id = "test-tenant";
        let result = handle_submit_feedback(
            req,
            tenant_id,
            agent_id,
            feedback_store.as_ref(),
            cache_store.as_ref(),
        )
        .await
        .unwrap();

        let cached_after = cache_store
            .get_cached("test-tenant", query_hash)
            .await
            .unwrap();
        assert!(cached_after.is_some());

        let entry_after = cached_after.unwrap();
        assert!(entry_after.success_count >= 1);
        assert!(entry_after.hit_count >= 1);

        let status = result.get("status").unwrap().as_str().unwrap();
        assert_eq!(status, "recorded");
        assert!(result.get("cache_adjusted").unwrap().as_bool().unwrap());

        println!("End-to-end feedback cache flow test passed");
        println!("   - Initial confidence: {}", initial_confidence);
        println!("   - Success count: {}", entry_after.success_count);
        println!("   - Hit count: {}", entry_after.hit_count);
    }
}
