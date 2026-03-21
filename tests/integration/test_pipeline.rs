//! Pipeline E2E tests using in-memory stores + mock LLM

use cogkos_core::audit::InMemoryAuditStore;
use cogkos_core::models::*;
use cogkos_ingest::{EmbeddingService, IngestionPipeline, UploadedFile};
use cogkos_llm::client::LlmClient;
use cogkos_store::*;
use std::sync::Arc;

/// Mock LLM client that returns deterministic embeddings
struct MockEmbeddingClient;

#[async_trait::async_trait]
impl LlmClient for MockEmbeddingClient {
    async fn chat(
        &self,
        _request: cogkos_llm::LlmRequest,
    ) -> cogkos_llm::Result<cogkos_llm::LlmResponse> {
        Ok(cogkos_llm::LlmResponse {
            content: String::new(),
            usage: None,
            finish_reason: Some("stop".to_string()),
        })
    }

    async fn chat_stream(
        &self,
        _request: cogkos_llm::LlmRequest,
    ) -> cogkos_llm::Result<
        std::pin::Pin<Box<dyn futures::Stream<Item = cogkos_llm::Result<String>> + Send>>,
    > {
        let stream = futures::stream::iter(vec![Ok(String::new())]);
        Ok(Box::pin(stream))
    }

    async fn embed(
        &self,
        texts: Vec<String>,
        _model: Option<String>,
    ) -> cogkos_llm::Result<Vec<Vec<f32>>> {
        // Return deterministic 4-dim vectors based on text hash
        Ok(texts
            .iter()
            .map(|t| {
                let h = t.len() as f32;
                vec![h * 0.01, 0.5 - h * 0.005, h * 0.002, 0.1]
            })
            .collect())
    }

    fn provider(&self) -> &'static str {
        "mock"
    }
}

struct MockObjectStore;

#[async_trait::async_trait]
impl ObjectStore for MockObjectStore {
    async fn upload(
        &self,
        key: &str,
        _data: &[u8],
        _content_type: &str,
    ) -> cogkos_core::Result<String> {
        Ok(format!("mock://{}", key))
    }
    async fn download(&self, _key: &str) -> cogkos_core::Result<Vec<u8>> {
        Ok(vec![])
    }
    async fn delete(&self, _key: &str) -> cogkos_core::Result<()> {
        Ok(())
    }
    async fn presigned_url(&self, _key: &str, _expiry_secs: u64) -> cogkos_core::Result<String> {
        Ok("mock://url".into())
    }
}

fn create_stores() -> Stores {
    Stores::new(
        Arc::new(InMemoryClaimStore::new()),
        Arc::new(InMemoryVectorStore::new()),
        Arc::new(InMemoryGraphStore::new()),
        Arc::new(InMemoryCacheStore::new()),
        Arc::new(InMemoryFeedbackStore::new()),
        Arc::new(MockObjectStore),
        Arc::new(InMemoryAuthStore::new()),
        Arc::new(InMemoryGapStore::new()),
        Arc::new(InMemoryAuditStore::new(1000)),
        Arc::new(InMemorySubscriptionStore::new()),
        Arc::new(NoopMemoryLayerStore),
        None,
    )
}

#[tokio::test]
async fn test_pipeline_ingest_text_document() {
    let stores = create_stores();
    let client: Arc<dyn LlmClient> = Arc::new(MockEmbeddingClient);
    let embedding_service = EmbeddingService::new(client);
    let pipeline = IngestionPipeline::new(embedding_service);

    let file = UploadedFile {
        filename: "test.txt".to_string(),
        content_type: "text/plain".to_string(),
        data: b"Rust provides memory safety without garbage collection. \
                The ownership system ensures no data races at compile time."
            .to_vec(),
        source: Claimant::System,
        tenant_id: "test-tenant".to_string(),
    };

    let result = pipeline
        .ingest(
            file,
            stores.claims.as_ref(),
            stores.graph.as_ref(),
            stores.vectors.as_ref(),
            stores.objects.as_ref(),
        )
        .await
        .unwrap();

    // Verify results
    assert!(
        !result.chunk_claim_ids.is_empty(),
        "Should have chunk claims"
    );
    assert!(result.novelty_score >= 0.0 && result.novelty_score <= 1.0);
    assert!(result.deep_classification.is_some());

    // Verify file claim is retrievable
    let file_claim = stores
        .claims
        .get_claim(result.file_claim_id, "test-tenant")
        .await
        .unwrap();
    assert!(file_claim.content.contains("test.txt"));

    // Verify chunk claims exist
    for chunk_id in &result.chunk_claim_ids {
        let chunk = stores
            .claims
            .get_claim(*chunk_id, "test-tenant")
            .await
            .unwrap();
        assert!(!chunk.content.is_empty());
    }
}

#[tokio::test]
async fn test_pipeline_rejects_oversized_file() {
    let stores = create_stores();
    let client: Arc<dyn LlmClient> = Arc::new(MockEmbeddingClient);
    let embedding_service = EmbeddingService::new(client);
    let pipeline = IngestionPipeline::new(embedding_service);

    // Create a file larger than 256MB
    let file = UploadedFile {
        filename: "huge.txt".to_string(),
        content_type: "text/plain".to_string(),
        data: vec![0u8; 256 * 1024 * 1024 + 1], // 256MB + 1 byte
        source: Claimant::System,
        tenant_id: "test-tenant".to_string(),
    };

    let result = pipeline
        .ingest(
            file,
            stores.claims.as_ref(),
            stores.graph.as_ref(),
            stores.vectors.as_ref(),
            stores.objects.as_ref(),
        )
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("too large"));
}

#[tokio::test]
async fn test_pipeline_ingest_markdown() {
    let stores = create_stores();
    let client: Arc<dyn LlmClient> = Arc::new(MockEmbeddingClient);
    let embedding_service = EmbeddingService::new(client);
    let pipeline = IngestionPipeline::new(embedding_service);

    let file = UploadedFile {
        filename: "notes.md".to_string(),
        content_type: "text/markdown".to_string(),
        data: b"# Machine Learning\n\nSupervised learning uses labeled data.\n\n## Deep Learning\n\nNeural networks with multiple layers."
            .to_vec(),
        source: Claimant::Human {
            user_id: "user-1".to_string(),
            role: "researcher".to_string(),
        },
        tenant_id: "research-org".to_string(),
    };

    let result = pipeline
        .ingest(
            file,
            stores.claims.as_ref(),
            stores.graph.as_ref(),
            stores.vectors.as_ref(),
            stores.objects.as_ref(),
        )
        .await
        .unwrap();

    assert!(!result.chunk_claim_ids.is_empty());

    // Verify domain metadata
    let file_claim = stores
        .claims
        .get_claim(result.file_claim_id, "research-org")
        .await
        .unwrap();
    assert!(file_claim.metadata.contains_key("domain"));
}
