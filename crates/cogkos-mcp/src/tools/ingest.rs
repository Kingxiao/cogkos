//! Ingest handlers: submit_experience, upload_document, get_meta_directory

use cogkos_core::models::*;
use cogkos_core::{CogKosError, Result};
use cogkos_ingest::{EmbeddingService, IngestionPipeline, UploadedFile};
use cogkos_llm::LlmClient;
use cogkos_store::{ClaimStore, GraphStore, ObjectStore, VectorStore};
use std::sync::Arc;

use super::helpers::{
    self, calculate_content_hash, calculate_hash, extract_domain, generate_query_vector,
};
use super::types::*;

/// Submit experience handler
pub async fn handle_submit_experience(
    req: SubmitExperienceRequest,
    tenant_id: &str,
    claim_store: Arc<dyn ClaimStore>,
    vector_store: Arc<dyn VectorStore>,
    graph_store: Arc<dyn GraphStore>,
    embedding_client: Option<Arc<dyn LlmClient>>,
) -> Result<serde_json::Value> {
    // Convert source
    let claimant = match req.source {
        SourceInfo::Human { user_id } => Claimant::Human {
            user_id,
            role: "user".to_string(),
        },
        SourceInfo::Agent { agent_id, model } => Claimant::Agent { agent_id, model },
        SourceInfo::External { source_name } => Claimant::ExternalPublic { source_name },
    };

    // Extract domain before consuming req.content
    let domain = extract_domain(&req.content);

    let provenance = ProvenanceRecord {
        source_id: match &claimant {
            Claimant::Agent { agent_id, .. } => agent_id.clone(),
            Claimant::Human { user_id, .. } => user_id.clone(),
            Claimant::ExternalPublic { source_name } => source_name.clone(),
            Claimant::System => "system".to_string(),
        },
        source_type: "experience".to_string(),
        ingestion_method: "mcp_submit".to_string(),
        original_url: None,
        audit_hash: calculate_hash(&req.content),
    };

    let access_envelope = AccessEnvelope::new(tenant_id).with_visibility(Visibility::Tenant);

    let mut claim = EpistemicClaim::new(
        req.content,
        tenant_id,
        req.node_type,
        claimant,
        access_envelope,
        provenance,
    );

    claim.confidence = req.confidence.unwrap_or(0.5);
    claim.t_valid_start = req.valid_from.unwrap_or(claim.t_valid_start);
    claim.t_valid_end = req.valid_to;
    claim.derived_from = req.related_to;

    // Save domain to metadata
    claim
        .metadata
        .insert("domain".to_string(), serde_json::Value::String(domain));

    // Memory layer + session scoping
    if let Some(ref layer) = req.memory_layer {
        claim.metadata.insert(
            "memory_layer".to_string(),
            serde_json::Value::String(layer.clone()),
        );
    }
    if let Some(ref sid) = req.session_id {
        claim.metadata.insert(
            "session_id".to_string(),
            serde_json::Value::String(sid.clone()),
        );
    }

    let t0 = std::time::Instant::now();
    let claim_id = claim_store.insert_claim(&claim).await?;
    tracing::info!(claim_id = %claim_id, pg_ms = t0.elapsed().as_millis() as u64, "Fast-track: claim persisted");

    // S2: Fast capture / slow consolidation
    // Return immediately after PG write, spawn background task for embedding + vector + graph + conflict
    let bg_claim = claim.clone();
    let bg_tenant = tenant_id.to_string();
    let bg_claim_id = claim_id;
    let bg_claim_store = Arc::clone(&claim_store);
    let bg_vector_store = Arc::clone(&vector_store);
    let bg_graph_store = Arc::clone(&graph_store);

    tokio::spawn(async move {
        let claim_store = bg_claim_store;
        let vector_store = bg_vector_store;
        let graph_store = bg_graph_store;
        let t_bg = std::time::Instant::now();

        // 1. Embedding
        let content_vector: Vec<f32> = if let Some(ref client) = embedding_client {
            let embedding_service = EmbeddingService::new(client.clone());
            match embedding_service.embed(&bg_claim.content).await {
                Ok(vec) => vec,
                Err(e) => {
                    tracing::warn!(claim_id = %bg_claim_id, error = %e, "Background embedding failed, using fallback");
                    generate_query_vector(&bg_claim.content, helpers::DEFAULT_FALLBACK_DIM)
                }
            }
        } else {
            generate_query_vector(&bg_claim.content, helpers::DEFAULT_FALLBACK_DIM)
        };
        let t_embed = t_bg.elapsed();

        // 2. Vector upsert
        let payload = serde_json::json!({
            "tenant_id": bg_tenant,
            "content": bg_claim.content,
            "node_type": bg_claim.node_type.as_db_str(),
        });
        if let Err(e) = vector_store
            .upsert(bg_claim_id, content_vector.clone(), payload)
            .await
        {
            tracing::warn!(claim_id = %bg_claim_id, error = %e, "Background vector upsert failed");
            // Mark for sleep-time reindex
            let mut patched = bg_claim.clone();
            patched
                .metadata
                .insert("needs_reindex".into(), serde_json::Value::Bool(true));
            claim_store.update_claim(&patched).await.ok();
        }

        // 3. Graph upsert
        if let Err(e) = graph_store.upsert_node(&bg_claim).await {
            tracing::warn!(claim_id = %bg_claim_id, error = %e, "Background graph upsert failed");
        }

        // 4. Semantic conflict detection + graph edge building
        if let Ok(matches) = vector_store
            .search(content_vector, &bg_tenant, 20, None)
            .await
        {
            let scored_matches: Vec<_> = matches
                .into_iter()
                .filter(|m| m.id != bg_claim_id && m.score > 0.3)
                .take(10)
                .collect();

            if !scored_matches.is_empty() {
                let mut candidate_claims: Vec<EpistemicClaim> = Vec::new();
                for m in &scored_matches {
                    if let Ok(c) = claim_store.get_claim(m.id, &bg_tenant).await {
                        candidate_claims.push(c);
                    }
                }

                for m in &scored_matches {
                    if m.score > 0.5 {
                        let relation = if m.score > 0.8 { "SIMILAR" } else { "RELATED" };
                        graph_store
                            .create_edge(bg_claim_id, m.id, relation, m.score)
                            .await
                            .ok();
                    }
                }

                let conflicts = cogkos_core::evolution::conflict::detect_conflicts_batch(
                    &bg_claim,
                    &candidate_claims,
                );
                for conflict in &conflicts {
                    claim_store.insert_conflict(conflict).await.ok();
                }

                tracing::debug!(
                    claim_id = %bg_claim_id,
                    edges = scored_matches.iter().filter(|m| m.score > 0.5).count(),
                    conflicts = conflicts.len(),
                    "Background: graph edges + conflict detection"
                );
            }
        }

        tracing::info!(
            claim_id = %bg_claim_id,
            embedding_ms = t_embed.as_millis() as u64,
            total_bg_ms = t_bg.elapsed().as_millis() as u64,
            "Background consolidation complete"
        );
    });

    let resp = serde_json::json!({
        "claim_id": claim_id.to_string(),
        "status": "accepted",
        "consolidation": "async",
        "estimated_consolidation_time": "5s"
    });
    Ok(resp)
}

/// Upload document handler
pub async fn handle_upload_document(
    req: UploadDocumentRequest,
    tenant_id: &str,
    claim_store: &dyn ClaimStore,
    graph_store: &dyn GraphStore,
    vector_store: &dyn VectorStore,
    object_store: &dyn ObjectStore,
    embedding_service: Option<EmbeddingService>,
) -> Result<DocumentUploadResponse> {
    // Decode base64 content
    let file_data =
        match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &req.content) {
            Ok(data) => data,
            Err(e) => return Err(CogKosError::InvalidInput(format!("Invalid base64: {}", e))),
        };

    // Calculate content hash for duplicate detection
    let content_hash = calculate_content_hash(&file_data);

    // Check for duplicate using hash-based key
    let hash_key = format!("{}/by_hash/{}", tenant_id, content_hash);

    if object_store.download(&hash_key).await.is_ok() {
        tracing::info!("Duplicate document detected, hash: {}", &content_hash[..16]);
        return Ok(DocumentUploadResponse {
            file_id: content_hash[..8].to_string(),
            status: "duplicate".to_string(),
            estimated_time: "0ms".to_string(),
            pipeline_id: None,
            is_duplicate: true,
        });
    }

    // Determine content type from filename
    let content_type = match req.filename.rsplit('.').next() {
        Some("pdf") => "application/pdf",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("md") => "text/markdown",
        Some("txt") => "text/plain",
        Some("csv") => "text/csv",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    };

    let file_id = uuid::Uuid::new_v4();

    let claimant = match req.source {
        SourceInfo::Human { user_id } => Claimant::Human {
            user_id,
            role: "user".to_string(),
        },
        SourceInfo::Agent { agent_id, model } => Claimant::Agent { agent_id, model },
        SourceInfo::External { source_name } => Claimant::ExternalPublic { source_name },
    };

    let hash_upload_key = format!("{}/by_hash/{}", tenant_id, content_hash);

    let (status, pipeline_id, estimated_time) = if req.auto_process {
        if let Some(embedding_svc) = embedding_service {
            if let Err(e) = object_store
                .upload(&hash_upload_key, &file_data, content_type)
                .await
            {
                tracing::warn!(key = %hash_upload_key, error = %e, "Failed to store hash-keyed duplicate detection copy");
            }

            let uploaded_file = UploadedFile {
                filename: req.filename.clone(),
                content_type: content_type.to_string(),
                data: file_data,
                source: claimant,
                tenant_id: tenant_id.to_string(),
            };

            let pipeline = IngestionPipeline::new(embedding_svc.clone());

            match pipeline
                .ingest(
                    uploaded_file,
                    claim_store,
                    graph_store,
                    vector_store,
                    object_store,
                )
                .await
            {
                Ok(result) => (
                    "completed",
                    Some(format!("pipe-{}", &file_id.to_string()[..8])),
                    format!("{}ms", result.chunk_claim_ids.len() * 100),
                ),
                Err(e) => {
                    tracing::error!("Ingestion pipeline error: {}", e);
                    (
                        "failed",
                        Some(format!("pipe-{}", &file_id.to_string()[..8])),
                        "0s".to_string(),
                    )
                }
            }
        } else {
            let s3_key = format!("{}/raw/{}/{}", tenant_id, file_id, req.filename);
            object_store
                .upload(&s3_key, &file_data, content_type)
                .await?;
            if let Err(e) = object_store
                .upload(&hash_upload_key, &file_data, content_type)
                .await
            {
                tracing::warn!(key = %hash_upload_key, error = %e, "Failed to store hash-keyed duplicate detection copy");
            }

            ("uploaded_no_processing", None, "0s".to_string())
        }
    } else {
        let s3_key = format!("{}/raw/{}/{}", tenant_id, file_id, req.filename);
        object_store
            .upload(&s3_key, &file_data, content_type)
            .await?;
        if let Err(e) = object_store
            .upload(&hash_upload_key, &file_data, content_type)
            .await
        {
            tracing::warn!(key = %hash_upload_key, error = %e, "Failed to store hash-keyed duplicate detection copy");
        }

        let domain = extract_domain(&req.filename);

        let provenance = ProvenanceRecord {
            source_id: "upload".to_string(),
            source_type: "document".to_string(),
            ingestion_method: "mcp_upload".to_string(),
            original_url: Some(format!("s3://{}", s3_key)),
            audit_hash: content_hash.clone(),
        };

        let access_envelope = AccessEnvelope::new(tenant_id).with_visibility(Visibility::Tenant);

        let mut claim = EpistemicClaim::new(
            format!("Document: {}", req.filename),
            tenant_id,
            NodeType::File,
            Claimant::System,
            access_envelope,
            provenance,
        );

        claim
            .metadata
            .insert("domain".to_string(), serde_json::Value::String(domain));
        claim.metadata.insert(
            "content_hash".to_string(),
            serde_json::Value::String(content_hash),
        );

        let _claim_id = claim_store.insert_claim(&claim).await?;

        ("uploaded", None, "0s".to_string())
    };

    Ok(DocumentUploadResponse {
        file_id: file_id.to_string(),
        status: status.to_string(),
        estimated_time,
        pipeline_id,
        is_duplicate: false,
    })
}

/// Get meta directory handler
#[allow(clippy::type_complexity)]
pub async fn handle_get_meta_directory(
    req: GetMetaDirectoryRequest,
    tenant_id: &str,
    claim_store: &dyn ClaimStore,
) -> Result<serde_json::Value> {
    let claims = claim_store
        .query_claims(tenant_id, &[])
        .await
        .map_err(|e| CogKosError::Database(format!("Failed to query claims: {}", e)))?;

    if claims.is_empty() {
        return Ok(serde_json::json!({
            "entries": [],
            "total_domains": 0,
            "total_claims": 0
        }));
    }

    let mut domain_map: std::collections::HashMap<
        String,
        (
            Vec<cogkos_core::models::EpistemicClaim>,
            std::collections::HashMap<String, usize>,
        ),
    > = std::collections::HashMap::new();

    for claim in &claims {
        let domain = claim
            .metadata
            .get("domain")
            .and_then(|v| v.as_str())
            .unwrap_or("general")
            .to_string();

        let entry = domain_map
            .entry(domain)
            .or_insert((Vec::new(), std::collections::HashMap::new()));
        entry.0.push(claim.clone());

        let node_type_str = format!("{:?}", claim.node_type);
        *entry.1.entry(node_type_str).or_insert(0) += 1;
    }

    let filtered_domains: Vec<(
        String,
        (
            Vec<cogkos_core::models::EpistemicClaim>,
            std::collections::HashMap<String, usize>,
        ),
    )> = if let Some(ref query_domain) = req.query_domain {
        domain_map
            .into_iter()
            .filter(|(d, _)| d == query_domain)
            .collect()
    } else {
        domain_map.into_iter().collect()
    };

    let mut entries: Vec<MetaDirectoryEntry> = filtered_domains
        .into_iter()
        .map(|(domain, (claims_in_domain, node_types))| {
            let claim_count = claims_in_domain.len();
            let avg_confidence: f64 = if claim_count > 0 {
                claims_in_domain.iter().map(|c| c.confidence).sum::<f64>() / claim_count as f64
            } else {
                0.0
            };

            let expertise_score = (claim_count as f64 * avg_confidence / 10.0).min(1.0);
            let latest_update = claims_in_domain.iter().map(|c| c.updated_at).max();

            MetaDirectoryEntry {
                domain,
                claim_count,
                expertise_score,
                node_types,
                avg_confidence,
                latest_update,
            }
        })
        .collect();

    entries.sort_by(|a, b| {
        b.expertise_score
            .partial_cmp(&a.expertise_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if let Some(min_score) = req.min_expertise_score {
        entries.retain(|e| e.expertise_score >= min_score);
    }

    let total_claims: usize = entries.iter().map(|e| e.claim_count).sum();
    let total_domains = entries.len();

    Ok(serde_json::json!({
        "entries": entries,
        "total_domains": total_domains,
        "total_claims": total_claims
    }))
}
