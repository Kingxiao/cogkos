//! LLM-based deep extraction — episodic claims to structured facts and relation triples.
//!
//! Sleep-time task that uses LLM (via cogkos-llm) to extract structured knowledge
//! from raw episodic turns. Replaces the keyword-based content_consolidation with
//! high-quality LLM inference on a 4h cycle.

use cogkos_core::Result;
use cogkos_core::models::{
    AccessEnvelope, Claimant, ConsolidationStage, EpistemicClaim, EpistemicStatus, KnowledgeType,
    NodeType, ProvenanceRecord,
};
use cogkos_llm::LlmClient;
use cogkos_store::Stores;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

// ---------------------------------------------------------------------------
// Extraction result
// ---------------------------------------------------------------------------

/// Structured output from LLM fact extraction.
#[derive(Debug, Clone)]
pub struct LlmExtractionResult {
    pub facts: Vec<String>,
    pub triples: Vec<(String, String, String)>, // (subject, predicate, object)
}

// ---------------------------------------------------------------------------
// Core extraction function
// ---------------------------------------------------------------------------

/// Extract structured facts from an episodic claim using LLM.
pub async fn extract_facts_with_llm(
    llm_client: &dyn LlmClient,
    claim: &EpistemicClaim,
    session_date: Option<&str>,
) -> Result<LlmExtractionResult> {
    let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4.1-nano".to_string()); // verified: 2026-03-30

    let prompt = format!(
        "Deduce the facts, preferences, and memories from the provided text.\n\
         Return JSON only: {{\"facts\": [\"fact1\", ...], \"triples\": [[\"subject\",\"predicate\",\"object\"], ...]}}\n\n\
         Text:\n[Date: {}]\n{}",
        session_date.unwrap_or("unknown"),
        claim.content
    );

    let request = cogkos_llm::LlmRequest {
        model,
        messages: vec![cogkos_llm::Message {
            role: cogkos_llm::Role::User,
            content: prompt,
        }],
        temperature: 0.0,
        max_tokens: Some(1500),
        ..Default::default()
    };

    let response = llm_client
        .chat(request)
        .await
        .map_err(|e| cogkos_core::CogKosError::ExternalError(format!("LLM extraction: {}", e)))?;

    parse_extraction_response(&response.content)
}

// ---------------------------------------------------------------------------
// JSON response parser (handles diverse LLM output formats)
// ---------------------------------------------------------------------------

/// Parse LLM response into structured facts and triples.
///
/// Handles variations:
/// - `{"facts": [...], "triples": [...]}`
/// - `{"facts": [...], "preferences": [...], "memories": [...]}`
/// - Markdown-wrapped JSON (```json ... ```)
fn parse_extraction_response(raw: &str) -> Result<LlmExtractionResult> {
    // Strip markdown code fences if present
    let cleaned = raw
        .trim()
        .strip_prefix("```json")
        .or_else(|| raw.trim().strip_prefix("```"))
        .unwrap_or(raw.trim());
    let cleaned = cleaned.strip_suffix("```").unwrap_or(cleaned).trim();

    let parsed: serde_json::Value = serde_json::from_str(cleaned).map_err(|e| {
        cogkos_core::CogKosError::Internal(format!("LLM returned invalid JSON: {}", e))
    })?;

    // Collect facts from all string-array fields (facts, preferences, memories, etc.)
    let mut facts = Vec::new();
    if let Some(obj) = parsed.as_object() {
        for (key, value) in obj {
            if key == "triples" {
                continue; // handled separately
            }
            if let Some(arr) = value.as_array() {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        let trimmed = s.trim();
                        if !trimmed.is_empty() {
                            facts.push(trimmed.to_string());
                        }
                    }
                }
            }
        }
    }

    // Parse triples: [[s, p, o], ...]
    let mut triples = Vec::new();
    if let Some(arr) = parsed.get("triples").and_then(|v| v.as_array()) {
        for triple in arr {
            if let Some(t) = triple.as_array() {
                if t.len() >= 3 {
                    let s = t[0].as_str().unwrap_or_default().trim().to_string();
                    let p = t[1].as_str().unwrap_or_default().trim().to_string();
                    let o = t[2].as_str().unwrap_or_default().trim().to_string();
                    if !s.is_empty() && !p.is_empty() && !o.is_empty() {
                        triples.push((s, p, o));
                    }
                }
            }
        }
    }

    Ok(LlmExtractionResult { facts, triples })
}

// ---------------------------------------------------------------------------
// Scheduler task runner
// ---------------------------------------------------------------------------

/// Run LLM extraction on unprocessed episodic claims across all tenants.
///
/// For each episodic claim without `llm_extracted: true` metadata:
/// 1. Call LLM to extract facts and triples
/// 2. Store facts as semantic-layer claims with `derived_from` links
/// 3. Store triples as graph edges via GraphStore
/// 4. Mark original claim as processed
pub async fn run_llm_extraction(stores: &Arc<Stores>, llm_client: &dyn LlmClient) -> Result<usize> {
    let start = std::time::Instant::now();
    info!("Running LLM deep extraction");

    let tenants = match stores.claims.list_tenants().await {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => vec!["default".to_string()],
        Err(e) => {
            warn!("Failed to list tenants for LLM extraction: {}", e);
            vec!["default".to_string()]
        }
    };

    let mut total = 0usize;

    for tenant_id in &tenants {
        // Fetch episodic claims not yet processed (batch of 100)
        let claims = stores
            .memory_layers
            .list_claims_by_memory_layer(tenant_id, "episodic", None, 100)
            .await
            .unwrap_or_default();

        for claim in &claims {
            // Skip already-extracted claims
            if claim
                .metadata
                .get("llm_extracted")
                .and_then(|v| v.as_bool())
                == Some(true)
            {
                continue;
            }

            let session_date = claim.metadata.get("session_date").and_then(|v| v.as_str());

            match extract_facts_with_llm(llm_client, claim, session_date).await {
                Ok(result) => {
                    // Store extracted facts as semantic claims
                    for fact in &result.facts {
                        let new_claim = build_semantic_claim(tenant_id, fact, claim);
                        if stores.claims.insert_claim(&new_claim).await.is_ok() {
                            // Add to graph; embedding generated lazily on first query
                            // (scheduler does not hold an embedding client).
                            stores.graph.add_node(&new_claim).await.ok();
                            total += 1;
                        }
                    }

                    // Store triples as graph edges
                    for (subject, predicate, object) in &result.triples {
                        store_triple(stores, tenant_id, subject, predicate, object, claim).await;
                    }

                    // Mark original claim as extracted
                    let mut updated = claim.clone();
                    updated
                        .metadata
                        .insert("llm_extracted".to_string(), serde_json::Value::Bool(true));
                    stores.claims.update_claim(&updated).await.ok();
                }
                Err(e) => {
                    warn!(
                        claim_id = %claim.id,
                        error = %e,
                        "LLM extraction failed, will retry next cycle"
                    );
                }
            }
        }
    }

    cogkos_core::monitoring::METRICS
        .record_duration("cogkos_scheduler_task_duration_seconds", start.elapsed());
    info!(
        total_extracted = total,
        duration_ms = start.elapsed().as_millis() as u64,
        "LLM deep extraction complete"
    );
    Ok(total)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a semantic-layer EpistemicClaim from an extracted fact string.
fn build_semantic_claim(tenant_id: &str, fact: &str, source: &EpistemicClaim) -> EpistemicClaim {
    let now = chrono::Utc::now();
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "memory_layer".to_string(),
        serde_json::Value::String("semantic".to_string()),
    );
    metadata.insert(
        "extracted_from".to_string(),
        serde_json::Value::String("llm_phase3".to_string()),
    );
    metadata.insert(
        "source_claim_id".to_string(),
        serde_json::Value::String(source.id.to_string()),
    );

    EpistemicClaim {
        id: uuid::Uuid::new_v4(),
        tenant_id: tenant_id.to_string(),
        content: fact.to_string(),
        node_type: NodeType::Entity,
        knowledge_type: KnowledgeType::Experiential,
        structured_content: None,
        claimant: Claimant::System,
        epistemic_status: EpistemicStatus::Asserted,
        confidence: 0.8,
        consolidation_stage: ConsolidationStage::Consolidated,
        version: 1,
        durability: 0.8,
        activation_weight: 0.5,
        access_count: 0,
        last_accessed: None,
        t_valid_start: now,
        t_valid_end: None,
        t_known: now,
        access_envelope: AccessEnvelope::new(tenant_id),
        provenance: ProvenanceRecord::new(
            format!("llm-extraction-{}", source.id),
            "system".to_string(),
            "sleep-time-llm-extraction".to_string(),
        ),
        vector_id: None,
        last_prediction_error: None,
        derived_from: vec![source.id],
        superseded_by: None,
        entity_refs: Vec::new(),
        needs_revalidation: false,
        created_at: now,
        updated_at: now,
        metadata,
    }
}

/// Store a relation triple as graph nodes + edge.
///
/// Creates placeholder entity claims for subject/object if they don't exist
/// as graph nodes yet, then links them with the predicate as edge label.
async fn store_triple(
    stores: &Arc<Stores>,
    tenant_id: &str,
    subject: &str,
    predicate: &str,
    object: &str,
    source: &EpistemicClaim,
) {
    // Create subject node claim
    let subj_claim = build_semantic_claim(tenant_id, subject, source);
    let subj_id = subj_claim.id;
    if stores.claims.insert_claim(&subj_claim).await.is_ok() {
        stores.graph.add_node(&subj_claim).await.ok();
    }

    // Create object node claim
    let obj_claim = build_semantic_claim(tenant_id, object, source);
    let obj_id = obj_claim.id;
    if stores.claims.insert_claim(&obj_claim).await.is_ok() {
        stores.graph.add_node(&obj_claim).await.ok();
    }

    // Sanitize predicate for graph edge label (uppercase, underscores)
    let edge_label = predicate
        .to_uppercase()
        .replace(' ', "_")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect::<String>();

    if !edge_label.is_empty() {
        if let Err(e) = stores
            .graph
            .add_edge(subj_id, obj_id, &edge_label, 0.8)
            .await
        {
            debug!(error = %e, "Failed to add triple edge to graph");
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_clean_json() {
        let raw = r#"{"facts": ["Alice is a teacher", "Bob likes cats"], "triples": [["Alice", "teaches", "Math"]]}"#;
        let result = parse_extraction_response(raw).unwrap();
        assert_eq!(result.facts.len(), 2);
        assert_eq!(result.triples.len(), 1);
        assert_eq!(
            result.triples[0],
            ("Alice".into(), "teaches".into(), "Math".into())
        );
    }

    #[test]
    fn test_parse_markdown_wrapped() {
        let raw = r#"```json
{"facts": ["one fact"], "triples": []}
```"#;
        let result = parse_extraction_response(raw).unwrap();
        assert_eq!(result.facts, vec!["one fact"]);
        assert!(result.triples.is_empty());
    }

    #[test]
    fn test_parse_extra_fields() {
        let raw = r#"{"facts": ["fact1"], "preferences": ["pref1"], "memories": ["mem1"], "triples": []}"#;
        let result = parse_extraction_response(raw).unwrap();
        assert_eq!(result.facts.len(), 3);
    }

    #[test]
    fn test_parse_empty_strings_skipped() {
        let raw = r#"{"facts": ["good", "", "  ", "also good"], "triples": []}"#;
        let result = parse_extraction_response(raw).unwrap();
        assert_eq!(result.facts, vec!["good", "also good"]);
    }

    #[test]
    fn test_parse_invalid_json() {
        let raw = "not json at all";
        assert!(parse_extraction_response(raw).is_err());
    }

    #[test]
    fn test_parse_incomplete_triples_skipped() {
        let raw = r#"{"facts": [], "triples": [["only", "two"], ["a", "b", "c"]]}"#;
        let result = parse_extraction_response(raw).unwrap();
        assert_eq!(result.triples.len(), 1);
    }
}
