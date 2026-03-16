use cogkos_core::Result;
use cogkos_llm::LlmClient;
use std::sync::Arc;

/// Embedding service for text vectorization
pub struct EmbeddingService {
    client: Arc<dyn LlmClient>,
    model: Option<String>,
}

impl EmbeddingService {
    /// Create new embedding service
    pub fn new(client: Arc<dyn LlmClient>) -> Self {
        Self {
            client,
            model: None,
        }
    }

    /// Create new embedding service with specific model
    pub fn with_model(client: Arc<dyn LlmClient>, model: impl Into<String>) -> Self {
        Self {
            client,
            model: Some(model.into()),
        }
    }

    /// Embed single text
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self
            .client
            .embed(vec![text.to_string()], self.model.clone())
            .await
            .map_err(|e| {
                cogkos_core::CogKosError::ExternalError(format!("Embedding failed: {}", e))
            })?;

        embeddings.into_iter().next().ok_or_else(|| {
            cogkos_core::CogKosError::ExternalError("No embedding returned".to_string())
        })
    }

    /// Embed multiple texts
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        self.client
            .embed(texts.to_vec(), self.model.clone())
            .await
            .map_err(|e| {
                cogkos_core::CogKosError::ExternalError(format!("Batch embedding failed: {}", e))
            })
    }
}

impl Clone for EmbeddingService {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            model: self.model.clone(),
        }
    }
}
