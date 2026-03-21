use cogkos_core::Result;
use cogkos_core::models::*;
use cogkos_llm::client::LlmClient;
use cogkos_store::{ClaimStore, GraphStore, ObjectStore, VectorStore};
use std::sync::Arc;
use uuid::Uuid;

use crate::deep_classifier::{DeepClassifier, DeepClassifierConfig};
use crate::embedding::EmbeddingService;
use crate::extractor::KnowledgeExtractor;
use crate::{DocumentType, IngestResult, ParserRegistry, TextChunk, UploadedFile, coarse_classify};

/// Maximum upload size: 256 MB
const MAX_UPLOAD_SIZE: usize = 256 * 1024 * 1024;

/// Semantic novelty threshold for Sleep-time aggregation trigger
/// If novelty > this threshold, the chunk is considered novel and needs Sleep-time aggregation
/// If novelty <= this threshold, the chunk is similar to existing knowledge and boosts confidence
const NOVELTY_THRESHOLD: f64 = 0.3;

/// Confidence boost factor for similar knowledge
#[allow(dead_code)]
const CONFIDENCE_BOOST: f64 = 0.05;

/// Extract domain from CoarseClassification result (Phase 1)
///
/// This follows the design document: use classifier.rs results to build domain
fn extract_domain_from_classification(
    classification: &crate::classifier::CoarseClassification,
) -> String {
    // Priority: industry > document_type > keywords
    // If industry is detected, use it as domain
    if let Some(ref industry) = classification.industry {
        return industry.clone();
    }

    // If document_type exists, use it
    if let Some(ref doc_type) = classification.document_type {
        return doc_type.clone();
    }

    // Use first keyword as domain if available
    if let Some(ref keyword) = classification.keywords.first() {
        return keyword.to_string();
    }

    // Default to "unclassified" (not "general" - following design doc)
    "unclassified".to_string()
}

/// Legacy function for backward compatibility
fn extract_domain(content: &str) -> String {
    // Use coarse_classify to get classification result
    let classification = coarse_classify(content, "");
    extract_domain_from_classification(&classification)
}

/// Ingestion pipeline
pub struct IngestionPipeline {
    parser_registry: ParserRegistry,
    embedding_service: EmbeddingService,
    deep_classifier: DeepClassifier,
    /// Optional LLM client for knowledge extraction (graceful degradation when absent)
    knowledge_extractor: Option<KnowledgeExtractor>,
}

impl IngestionPipeline {
    /// Create new pipeline without LLM knowledge extraction
    pub fn new(embedding_service: EmbeddingService) -> Self {
        Self {
            parser_registry: ParserRegistry::new(),
            embedding_service,
            deep_classifier: DeepClassifier::new(DeepClassifierConfig::default()),
            knowledge_extractor: None,
        }
    }

    /// Create new pipeline with optional LLM client for knowledge extraction
    pub fn with_llm(
        embedding_service: EmbeddingService,
        llm_client: Option<Arc<dyn LlmClient>>,
    ) -> Self {
        Self {
            parser_registry: ParserRegistry::new(),
            embedding_service,
            deep_classifier: DeepClassifier::new(DeepClassifierConfig::default()),
            knowledge_extractor: llm_client.map(KnowledgeExtractor::new),
        }
    }

    /// Ingest a document
    #[tracing::instrument(skip_all, fields(filename = %file.filename, tenant = %file.tenant_id))]
    pub async fn ingest(
        &self,
        file: UploadedFile,
        claim_store: &dyn ClaimStore,
        graph_store: &dyn GraphStore,
        vector_store: &dyn VectorStore,
        object_store: &dyn ObjectStore,
    ) -> Result<IngestResult> {
        let ingest_start = std::time::Instant::now();

        // Validate upload size before processing
        if file.data.len() > MAX_UPLOAD_SIZE {
            return Err(cogkos_core::CogKosError::InvalidInput(format!(
                "File too large: {} bytes (max {} bytes)",
                file.data.len(),
                MAX_UPLOAD_SIZE
            )));
        }

        let doc_type = DocumentType::from_filename(&file.filename);
        let tenant_id = file.tenant_id.clone();

        // 1. Store raw file in S3
        let s3_key = format!("raw/{}/{}", tenant_id, file.filename);
        object_store
            .upload(&s3_key, &file.data, doc_type.mime_type())
            .await?;

        // 2. Create File claim for the document
        let provenance = ProvenanceRecord {
            source_id: format!("upload:{}", file.filename),
            source_type: "upload".to_string(),
            ingestion_method: "pipeline".to_string(),
            original_url: Some(format!("s3://{}", s3_key)),
            audit_hash: calculate_hash(&file.data),
        };

        // Infer visibility based on claimant source
        let access_envelope = AccessEnvelope::from_claimant(&tenant_id, &file.source);

        let mut file_claim = EpistemicClaim::new(
            format!("Document: {}", file.filename),
            tenant_id.clone(),
            NodeType::File,
            file.source.clone(),
            access_envelope.clone(),
            provenance,
        );

        // 3. Classify document
        let classification = coarse_classify(&file.filename, "");
        file_claim.content = format!(
            "{} [Type: {:?}]",
            file.filename, classification.document_type
        );

        // Extract and store domain in metadata
        let domain = extract_domain(&file.filename);
        file_claim.metadata.insert(
            "domain".to_string(),
            serde_json::Value::String(domain.clone()),
        );

        // Store content hash for duplicate detection
        let content_hash = calculate_hash(&file.data);
        file_claim.metadata.insert(
            "content_hash".to_string(),
            serde_json::Value::String(content_hash),
        );

        let file_claim_id = claim_store.insert_claim(&file_claim).await?;

        // 4. Parse document into chunks
        let chunks = self
            .parser_registry
            .parse(&file.data, &file.filename)
            .await?;

        // 4b. Deep classification (Phase 3)
        // Combine all chunk content for deep classification
        let full_content: String = chunks
            .iter()
            .map(|c| c.content.clone())
            .collect::<Vec<_>>()
            .join("\n");

        // Run deep classification (rule-based by default, LLM if enabled)
        let deep_classification = self.deep_classifier.classify(&full_content, None).await;

        // Store deep classification results in file claim metadata
        if let Some(ref industry) = deep_classification.industry {
            file_claim.metadata.insert(
                "industry".to_string(),
                serde_json::Value::String(industry.clone()),
            );
        }

        if let Some(ref doc_type) = deep_classification.document_type {
            file_claim.metadata.insert(
                "document_type_deep".to_string(),
                serde_json::Value::String(doc_type.clone()),
            );
        }

        // Store deep classification fields as JSON metadata
        macro_rules! store_meta {
            ($key:expr, $val:expr) => {
                if !$val.is_empty() {
                    match serde_json::to_value(&$val) {
                        Ok(v) => { file_claim.metadata.insert($key.to_string(), v); }
                        Err(e) => { tracing::warn!(field = $key, error = %e, "Failed to serialize deep classification field"); }
                    }
                }
            };
        }
        store_meta!("entities", deep_classification.entities);
        store_meta!("predictions", deep_classification.predictions);
        store_meta!("data_points", deep_classification.data_points);
        store_meta!("methodologies", deep_classification.methodologies);

        // Store confidence score
        file_claim.metadata.insert(
            "deep_classification_confidence".to_string(),
            serde_json::Value::Number(
                serde_json::Number::from_f64(deep_classification.confidence)
                    .unwrap_or(serde_json::Number::from(0)),
            ),
        );

        // 5. Process each chunk — with optional LLM knowledge extraction
        let mut chunk_claim_ids = Vec::new();
        let mut all_claims = Vec::new();
        let mut extracted_claim_ids = Vec::new();

        for chunk in &chunks {
            // Always create the raw text chunk claim (for provenance/traceability)
            let chunk_claim = self.chunk_to_claim(chunk, &file, &access_envelope, file_claim_id);
            let raw_chunk_id = chunk_claim.id;

            chunk_claim_ids.push(chunk_claim.id);
            all_claims.push(chunk_claim);

            // 5b. LLM knowledge extraction (when extractor is available)
            if let Some(ref extractor) = self.knowledge_extractor {
                let chunk_domain = domain.clone();
                if let Some(knowledge) = extractor.extract(&chunk.content, &chunk_domain).await {
                    if !knowledge.is_empty() {
                        tracing::info!(
                            chunk_id = %raw_chunk_id,
                            facts = knowledge.facts.len(),
                            decisions = knowledge.decisions.len(),
                            predictions = knowledge.predictions.len(),
                            relations = knowledge.relations.len(),
                            "LLM knowledge extraction complete"
                        );

                        // Create claims for each extracted item
                        for item in knowledge.all_items() {
                            let extracted_claim = self.extracted_item_to_claim(
                                item,
                                &file,
                                &access_envelope,
                                raw_chunk_id,
                                &chunk_domain,
                            );
                            extracted_claim_ids.push(extracted_claim.id);
                            chunk_claim_ids.push(extracted_claim.id);
                            all_claims.push(extracted_claim);
                        }

                        // Store relation metadata on raw chunk for graph building
                        if !knowledge.relations.is_empty() {
                            if let Some(raw_claim) =
                                all_claims.iter_mut().find(|c| c.id == raw_chunk_id)
                            {
                                let relations_json: Vec<serde_json::Value> = knowledge
                                    .relations
                                    .iter()
                                    .map(|r| {
                                        serde_json::json!({
                                            "subject": r.subject,
                                            "relation": r.relation,
                                            "object": r.object,
                                        })
                                    })
                                    .collect();
                                raw_claim.metadata.insert(
                                    "extracted_relations".to_string(),
                                    serde_json::Value::Array(relations_json),
                                );
                            }
                        }
                    }
                }
            }
        }

        // 6. Embed and store all claims (raw chunks + extracted items)
        let texts: Vec<String> = all_claims.iter().map(|c| c.content.clone()).collect();
        let embeddings = self.embedding_service.embed_batch(&texts).await?;

        let mut total_novelty = 0.0;
        for (i, (chunk_claim, embedding)) in all_claims.iter().zip(embeddings.iter()).enumerate() {
            // Insert claim — if this fails, skip this chunk entirely
            if let Err(e) = claim_store.insert_claim(chunk_claim).await {
                tracing::error!(
                    claim_id = %chunk_claim.id,
                    error = %e,
                    "Failed to insert chunk claim, skipping chunk {}",
                    i
                );
                continue;
            }

            // Store in vector DB
            let payload = serde_json::json!({
                "claim_id": chunk_claim.id.to_string(),
                "tenant_id": tenant_id,
                "content": chunk_claim.content,
                "node_type": format!("{:?}", chunk_claim.node_type),
            });

            if let Err(e) = vector_store
                .upsert(chunk_claim.id, embedding.clone(), payload)
                .await
            {
                tracing::warn!(claim_id = %chunk_claim.id, error = %e, "Vector upsert failed, removing orphan claim");
                if let Err(del_err) = claim_store.delete_claim(chunk_claim.id, &tenant_id).await {
                    tracing::error!(claim_id = %chunk_claim.id, error = %del_err, "Failed to delete orphan claim after vector upsert failure");
                }
                continue;
            }

            // Calculate novelty
            let novelty = vector_store
                .calculate_novelty(embedding.clone(), &tenant_id)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "Failed to calculate novelty, defaulting to 0.5");
                    0.5
                });
            total_novelty += novelty;

            // Semantic distance threshold trigger logic
            if novelty > NOVELTY_THRESHOLD {
                // Novel knowledge - trigger Sleep-time aggregation
                // Mark this chunk as needing Sleep-time aggregation via metadata AND stage
                let mut updated_claim = chunk_claim.clone();
                updated_claim.metadata.insert(
                    "needs_sleep_time_aggregation".to_string(),
                    serde_json::Value::Bool(true),
                );
                updated_claim.metadata.insert(
                    "novelty_score".to_string(),
                    serde_json::Value::Number(
                        serde_json::Number::from_f64(novelty)
                            .unwrap_or(serde_json::Number::from(0)),
                    ),
                );
                // Set consolidation stage to trigger Sleep-time consolidation task
                updated_claim.consolidation_stage = ConsolidationStage::PendingAggregation;
                claim_store.update_claim(&updated_claim).await?;
            } else {
                // Similar knowledge exists - queue confidence boost task via evolution engine
                // Find similar claims and mark them for confidence boost
                // This goes through the task queue instead of direct DB update
                let similar = claim_store
                    .search_claims(&tenant_id, &chunk_claim.content, 5)
                    .await?;

                // Collect IDs of similar claims that need boosting
                let mut similar_ids_to_boost: Vec<String> = Vec::new();
                for similar_claim in similar {
                    if similar_claim.confidence < 0.95 {
                        similar_ids_to_boost.push(similar_claim.id.to_string());
                    }
                }

                // Mark this chunk as needing confidence boost
                // The Sleep-time scheduler will process this and boost the similar claims
                let mut updated_claim = chunk_claim.clone();
                updated_claim.metadata.insert(
                    "needs_confidence_boost".to_string(),
                    serde_json::Value::Bool(true),
                );
                updated_claim.metadata.insert(
                    "similar_claim_ids_to_boost".to_string(),
                    serde_json::Value::Array(
                        similar_ids_to_boost
                            .iter()
                            .map(|s| serde_json::Value::String(s.clone()))
                            .collect(),
                    ),
                );
                updated_claim.metadata.insert(
                    "novelty_score".to_string(),
                    serde_json::Value::Number(
                        serde_json::Number::from_f64(novelty)
                            .unwrap_or(serde_json::Number::from(0)),
                    ),
                );
                claim_store.update_claim(&updated_claim).await?;
            }

            // Add to graph (non-fatal — claim is already persisted in PostgreSQL)
            if let Err(e) = graph_store.upsert_node(chunk_claim).await {
                tracing::warn!(claim_id = %chunk_claim.id, error = %e, "Failed to upsert graph node");
            }

            // Link to file claim
            if i == 0
                && let Err(e) = graph_store
                    .create_edge(file_claim_id, chunk_claim.id, "CONTAINS", 1.0)
                    .await
            {
                tracing::warn!(error = %e, "Failed to create graph edge");
            }
        }

        // 7. Detect conflicts — batch approach to avoid N+1
        let mut all_conflicts = Vec::new();
        if !all_claims.is_empty() {
            // Use first 3 chunks as representative sample for a single search
            let combined_query: String = all_claims
                .iter()
                .take(3)
                .map(|c| c.content.as_str())
                .collect::<Vec<_>>()
                .join(" ");

            let candidates = claim_store
                .search_claims(&tenant_id, &combined_query, 20)
                .await?;

            // Check each new claim against the candidates
            for claim in &all_claims {
                let conflicts = cogkos_core::evolution::detect_conflicts_batch(claim, &candidates);
                all_conflicts.extend(conflicts);
            }
        }

        // Store conflicts
        for conflict in &all_conflicts {
            claim_store.insert_conflict(conflict).await?;
        }

        // 8. Update file claim with vector reference
        file_claim.derived_from = chunk_claim_ids.clone();
        claim_store.update_claim(&file_claim).await?;

        let avg_novelty = if !all_claims.is_empty() {
            total_novelty / all_claims.len() as f64
        } else {
            1.0
        };

        cogkos_core::monitoring::METRICS
            .record_duration("cogkos_ingest_duration_seconds", ingest_start.elapsed());
        cogkos_core::monitoring::METRICS.inc_counter("cogkos_ingest_total", 1);

        Ok(IngestResult {
            file_claim_id,
            chunk_claim_ids,
            conflicts_detected: all_conflicts,
            novelty_score: avg_novelty,
            deep_classification: Some(deep_classification),
        })
    }

    /// Convert text chunk to claim
    fn chunk_to_claim(
        &self,
        chunk: &TextChunk,
        file: &UploadedFile,
        access_envelope: &AccessEnvelope,
        parent_id: Uuid,
    ) -> EpistemicClaim {
        let provenance = ProvenanceRecord {
            source_id: format!("doc:{}", file.filename),
            source_type: "document".to_string(),
            ingestion_method: "chunk".to_string(),
            original_url: None,
            audit_hash: calculate_hash(chunk.content.as_bytes()),
        };

        let mut claim = EpistemicClaim::new(
            chunk.content.clone(),
            file.tenant_id.clone(),
            NodeType::Entity, // Using rule-based classification for now
            file.source.clone(),
            access_envelope.clone(),
            provenance,
        );

        claim.derived_from = vec![parent_id];
        claim.consolidation_stage = ConsolidationStage::FastTrack;

        // Extract domain from chunk content and store in metadata
        let domain = extract_domain(&chunk.content);
        claim
            .metadata
            .insert("domain".to_string(), serde_json::Value::String(domain));

        claim
    }

    /// Convert an LLM-extracted knowledge item to a claim
    fn extracted_item_to_claim(
        &self,
        item: &crate::extractor::ExtractedItem,
        file: &UploadedFile,
        access_envelope: &AccessEnvelope,
        parent_chunk_id: Uuid,
        domain: &str,
    ) -> EpistemicClaim {
        let provenance = ProvenanceRecord {
            source_id: format!("doc:{}", file.filename),
            source_type: "document".to_string(),
            ingestion_method: "llm_extraction".to_string(),
            original_url: None,
            audit_hash: calculate_hash(item.content.as_bytes()),
        };

        let mut claim = EpistemicClaim::new(
            item.content.clone(),
            file.tenant_id.clone(),
            item.node_type.clone(),
            file.source.clone(),
            access_envelope.clone(),
            provenance,
        );

        claim.confidence = item.confidence;
        claim.derived_from = vec![parent_chunk_id];
        claim.consolidation_stage = ConsolidationStage::FastTrack;

        claim.metadata.insert(
            "domain".to_string(),
            serde_json::Value::String(domain.to_string()),
        );
        claim.metadata.insert(
            "extraction_method".to_string(),
            serde_json::Value::String("llm".to_string()),
        );
        claim.metadata.insert(
            "source_chunk_id".to_string(),
            serde_json::Value::String(parent_chunk_id.to_string()),
        );

        claim
    }
}

/// Calculate simple hash for audit
fn calculate_hash(data: &[u8]) -> String {
    use hex::encode;
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(data);
    encode(hasher.finalize())
}
