//! L9 Ingestion Pipeline Adapter
//!
//! Provides adapter for sending external documents to the L9 ingestion pipeline.
//! Converts ExternalDocument to EpistemicClaim for knowledge base storage.

use crate::{
    error::ExternalError,
    types::{ExternalDocument, SourceType},
    Result,
};
use cogkos_core::models::{
    AccessEnvelope, Claimant, ConsolidationStage, EpistemicClaim, NodeType, ProvenanceRecord,
    Visibility,
};
use std::sync::Arc;

/// Trait for L9 ingestion pipeline
///
/// Implement this trait to connect external documents to the knowledge base.
#[async_trait::async_trait]
pub trait L9IngestionPipeline: Send + Sync {
    /// Ingest a document into the knowledge base
    ///
    /// # Arguments
    /// * `document` - The external document to ingest
    /// * `tenant_id` - The tenant ID for multi-tenancy isolation
    ///
    /// # Returns
    /// * `Ok(EpistemicClaim)` - The ingested claim with assigned ID
    /// * `Err(ExternalError)` - If ingestion fails
    async fn ingest_document(
        &self,
        document: ExternalDocument,
        tenant_id: String,
    ) -> Result<EpistemicClaim>;

    /// Ingest multiple documents in batch
    ///
    /// # Arguments
    /// * `documents` - Vector of documents to ingest
    /// * `tenant_id` - The tenant ID for multi-tenancy isolation
    ///
    /// # Returns
    /// * `Ok(Vec<EpistemicClaim>)` - The ingested claims
    /// * `Err(ExternalError)` - If ingestion fails
    async fn ingest_batch(
        &self,
        documents: Vec<ExternalDocument>,
        tenant_id: String,
    ) -> Result<Vec<EpistemicClaim>> {
        let mut claims = Vec::new();
        for doc in documents {
            match self.ingest_document(doc, tenant_id.clone()).await {
                Ok(claim) => claims.push(claim),
                Err(e) => tracing::error!("Failed to ingest document: {}", e),
            }
        }
        Ok(claims)
    }
}

/// Adapter for converting ExternalDocument to EpistemicClaim
pub struct L9DocumentAdapter {
    /// Default tenant ID if not specified
    default_tenant_id: String,
    /// Default claimant for external documents
    default_claimant: Claimant,
}

impl L9DocumentAdapter {
    /// Create a new L9 adapter
    pub fn new(default_tenant_id: impl Into<String>) -> Self {
        Self {
            default_tenant_id: default_tenant_id.into(),
            default_claimant: Claimant::System,
        }
    }

    /// Create with custom claimant
    pub fn with_claimant(
        default_tenant_id: impl Into<String>,
        claimant: Claimant,
    ) -> Self {
        Self {
            default_tenant_id: default_tenant_id.into(),
            default_claimant: claimant,
        }
    }

    /// Convert ExternalDocument to EpistemicClaim
    ///
    /// This is the core conversion that prepares a document for L9 ingestion.
    pub fn to_claim(
        &self,
        doc: &ExternalDocument,
        tenant_id: Option<String>,
    ) -> EpistemicClaim {
        let tenant_id = tenant_id.unwrap_or_else(|| self.default_tenant_id.clone());

        // Determine node type based on source type
        let node_type = self.infer_node_type(&doc.source_type);

        // Build provenance record
        let provenance = ProvenanceRecord {
            source_id: doc.id.clone(),
            source_type: format!("{:?}", doc.source_type),
            ingestion_method: "external_subscription".to_string(),
            original_url: Some(doc.url.clone()),
            audit_hash: self.calculate_hash(doc),
        };

        // Create access envelope with tenant visibility
        let access_envelope = AccessEnvelope::new(&tenant_id)
            .with_visibility(Visibility::Tenant);

        // Determine claimant based on source
        let claimant = doc.authors.first()
            .map(|author| Claimant::ExternalPublic {
                source_name: author.clone(),
            })
            .unwrap_or_else(|| self.default_claimant.clone());

        // Build content with metadata
        let content = format!(
            "{}\n\n{}",
            doc.title,
            doc.content
        );

        let mut claim = EpistemicClaim::new(
            content,
            tenant_id,
            node_type,
            claimant,
            access_envelope,
            provenance,
        );

        // Set confidence from document
        claim.confidence = doc.confidence.clamp(0.0, 1.0);

        // Set consolidation stage based on confidence
        claim.consolidation_stage = if claim.confidence >= 0.8 {
            ConsolidationStage::FastTrack
        } else {
            ConsolidationStage::Consolidated
        };

        // Add metadata
        if let Ok(metadata_json) = serde_json::to_value(doc) {
            if let serde_json::Value::Object(map) = metadata_json {
                for (key, value) in map {
                    claim.metadata.insert(key, value);
                }
            }
        }

        // Add tags from document
        for tag in &doc.tags {
            claim.metadata.insert(
                format!("tag_{}", tag),
                serde_json::json!(true),
            );
        }

        claim
    }

    /// Convert multiple documents to claims
    pub fn to_claims(
        &self,
        docs: &[ExternalDocument],
        tenant_id: Option<String>,
    ) -> Vec<EpistemicClaim> {
        docs.iter()
            .map(|doc| self.to_claim(doc, tenant_id.clone()))
            .collect()
    }

    /// Infer node type from source type
    fn infer_node_type(&self, source_type: &SourceType) -> NodeType {
        match source_type {
            SourceType::RssFeed => NodeType::Entity,
            SourceType::Wikipedia => NodeType::Entity,
            SourceType::Arxiv => NodeType::Insight,
            SourceType::SearchEngine => NodeType::Entity,
            SourceType::WebPage => NodeType::Entity,
            SourceType::ApiResponse => NodeType::Event,
        }
    }

    /// Calculate audit hash for document
    fn calculate_hash(&self, doc: &ExternalDocument) -> String {
        use sha2::{Digest, Sha256};
        use hex::encode;

        let mut hasher = Sha256::new();
        hasher.update(doc.id.as_bytes());
        hasher.update(doc.title.as_bytes());
        hasher.update(doc.url.as_bytes());
        hasher.update(doc.fetched_at.to_rfc3339().as_bytes());
        encode(hasher.finalize())
    }
}

impl Default for L9DocumentAdapter {
    fn default() -> Self {
        Self::new("default")
    }
}

/// RSS-specific L9 ingestion helper
pub struct RssL9Ingestor {
    adapter: L9DocumentAdapter,
    pipeline: Arc<dyn L9IngestionPipeline>,
}

impl RssL9Ingestor {
    /// Create new RSS L9 ingestor
    pub fn new(
        tenant_id: impl Into<String>,
        pipeline: Arc<dyn L9IngestionPipeline>,
    ) -> Self {
        let adapter = L9DocumentAdapter::new(tenant_id);
        Self { adapter, pipeline }
    }

    /// Create with custom claimant
    pub fn with_claimant(
        tenant_id: impl Into<String>,
        claimant: Claimant,
        pipeline: Arc<dyn L9IngestionPipeline>,
    ) -> Self {
        let adapter = L9DocumentAdapter::with_claimant(tenant_id, claimant);
        Self { adapter, pipeline }
    }

    /// Ingest RSS documents to L9
    pub async fn ingest_rss_documents(
        &self,
        documents: Vec<ExternalDocument>,
    ) -> Result<Vec<EpistemicClaim>> {
        let tenant_id = self.adapter.default_tenant_id.clone();
        self.pipeline.ingest_batch(documents, tenant_id).await
    }

    /// Get the adapter reference
    pub fn adapter(&self) -> &L9DocumentAdapter {
        &self.adapter
    }
}

/// Mock L9 pipeline for testing
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::sync::Mutex;

    pub struct MockL9Pipeline {
        ingested: Mutex<Vec<EpistemicClaim>>,
    }

    impl MockL9Pipeline {
        pub fn new() -> Self {
            Self {
                ingested: Mutex::new(Vec::new()),
            }
        }

        pub fn get_ingested(&self) -> Vec<EpistemicClaim> {
            self.ingested.lock().unwrap().clone()
        }

        pub fn clear(&self) {
            self.ingested.lock().unwrap().clear();
        }
    }

    #[async_trait::async_trait]
    impl L9IngestionPipeline for MockL9Pipeline {
        async fn ingest_document(
            &self,
            document: ExternalDocument,
            tenant_id: String,
        ) -> Result<EpistemicClaim> {
            let adapter = L9DocumentAdapter::new(tenant_id);
            let claim = adapter.to_claim(&document, None);
            self.ingested.lock().unwrap().push(claim.clone());
            Ok(claim)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_document() -> ExternalDocument {
        ExternalDocument {
            id: "rss:test:1".to_string(),
            title: "Test Article".to_string(),
            content: "This is a test article content.".to_string(),
            url: "https://example.com/article/1".to_string(),
            source: "Test RSS Feed".to_string(),
            source_type: SourceType::RssFeed,
            published_at: Some(Utc::now()),
            authors: vec!["Test Author".to_string()],
            tags: vec!["tech".to_string(), "rust".to_string()],
            metadata: serde_json::json!({
                "feed_url": "https://example.com/feed.xml",
            }),
            confidence: 0.9,
            fetched_at: Utc::now(),
        }
    }

    #[test]
    fn test_adapter_conversion() {
        let adapter = L9DocumentAdapter::new("test-tenant");
        let doc = create_test_document();

        let claim = adapter.to_claim(&doc, None);

        assert_eq!(claim.tenant_id, "test-tenant");
        assert_eq!(claim.confidence, 0.9);
        assert!(matches!(claim.node_type, NodeType::Entity));
        assert!(matches!(claim.consolidation_stage, ConsolidationStage::FastTrack));
        assert!(claim.content.contains("Test Article"));
        assert!(claim.content.contains("test article content"));
    }

    #[test]
    fn test_node_type_inference() {
        let adapter = L9DocumentAdapter::new("test");

        assert!(matches!(
            adapter.infer_node_type(&SourceType::RssFeed),
            NodeType::Entity
        ));
        assert!(matches!(
            adapter.infer_node_type(&SourceType::Arxiv),
            NodeType::Insight
        ));
        assert!(matches!(
            adapter.infer_node_type(&SourceType::ApiResponse),
            NodeType::Event
        ));
    }

    #[test]
    fn test_hash_calculation() {
        let adapter = L9DocumentAdapter::new("test");
        let doc = create_test_document();

        let hash1 = adapter.calculate_hash(&doc);
        let hash2 = adapter.calculate_hash(&doc);

        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64); // SHA-256 hex string length
    }

    #[test]
    fn test_custom_tenant() {
        let adapter = L9DocumentAdapter::new("default-tenant");
        let doc = create_test_document();

        let claim = adapter.to_claim(&doc, Some("custom-tenant".to_string()));
        assert_eq!(claim.tenant_id, "custom-tenant");
    }

    #[test]
    fn test_claimant_from_author() {
        let adapter = L9DocumentAdapter::new("test");
        let mut doc = create_test_document();
        doc.authors = vec!["John Doe".to_string()];

        let claim = adapter.to_claim(&doc, None);

        match claim.claimant {
            Claimant::ExternalPublic { source_name } => {
                assert_eq!(source_name, "John Doe");
            }
            _ => panic!("Expected ExternalPublic claimant"),
        }
    }

    #[test]
    fn test_low_confidence_consolidation() {
        let adapter = L9DocumentAdapter::new("test");
        let mut doc = create_test_document();
        doc.confidence = 0.5;

        let claim = adapter.to_claim(&doc, None);

        assert_eq!(claim.confidence, 0.5);
        assert!(matches!(claim.consolidation_stage, ConsolidationStage::Consolidated));
    }
}
