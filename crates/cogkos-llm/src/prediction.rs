//! Lightweight prediction based on belief context
//!
//! This module implements S1: "memory is prediction" - generating predictions
//! based on the current belief context without complex reasoning.

use cogkos_core::models::{BeliefSummary, PredictionMethod, PredictionResult};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{Message, Role};

/// Prediction service for generating lightweight predictions
pub struct PredictionService {
    llm_client: Arc<dyn crate::LlmClient>,
    default_model: String,
}

impl PredictionService {
    /// Create a new prediction service
    pub fn new(llm_client: Arc<dyn crate::LlmClient>, default_model: impl Into<String>) -> Self {
        Self {
            llm_client,
            default_model: default_model.into(),
        }
    }

    /// Generate a prediction based on belief context
    ///
    /// This implements lightweight prediction as described in ARCHITECTURE.md S1:
    /// - Uses the belief context to generate predictions
    /// - Does not use complex reasoning
    /// - Can be replaced with dedicated models later
    pub async fn predict(
        &self,
        query: &str,
        beliefs: &[BeliefSummary],
        method: PredictionMethod,
    ) -> crate::Result<PredictionResult> {
        match method {
            PredictionMethod::LlmBeliefContext => {
                self.predict_with_llm_context(query, beliefs).await
            }
            PredictionMethod::DedicatedModel => {
                // NOTE: Dedicated model prediction not yet implemented
                self.predict_statistical_fallback(beliefs).await
            }
            PredictionMethod::StatisticalTrend => self.predict_statistical_fallback(beliefs).await,
        }
    }

    /// Generate prediction using LLM with belief context
    async fn predict_with_llm_context(
        &self,
        query: &str,
        beliefs: &[BeliefSummary],
    ) -> crate::Result<PredictionResult> {
        // Build context from beliefs
        let context = self.build_belief_context(beliefs);

        // Build prompt for prediction
        let system_prompt = r#"You are a prediction assistant for a knowledge management system.
Your task is to generate a lightweight prediction based on the provided belief context.

Rules:
1. Keep predictions concise and actionable
2. Base predictions only on the provided context
3. If insufficient context, acknowledge uncertainty
4. Do not make up information not supported by the context"#;

        let user_prompt = format!(
            "Query: {}\n\nRelevant Beliefs:\n{}\n\nBased on the above, what prediction can you make about {}?",
            query, context, query
        );

        let messages = vec![
            Message {
                role: Role::System,
                content: system_prompt.to_string(),
            },
            Message {
                role: Role::User,
                content: user_prompt,
            },
        ];

        let request = crate::LlmRequest {
            model: self.default_model.clone(),
            messages,
            temperature: 0.3, // Low temperature for more focused predictions
            max_tokens: Some(256),
            top_p: None,
            stop_sequences: vec![],
        };

        let response = self.llm_client.chat(request).await?;

        // Calculate confidence based on belief confidence and response quality
        let avg_belief_confidence = if !beliefs.is_empty() {
            beliefs.iter().map(|b| b.confidence).sum::<f64>() / beliefs.len() as f64
        } else {
            0.3
        };

        let confidence = (avg_belief_confidence * 0.7 + 0.3).min(1.0);

        // Flatten claim IDs from beliefs
        let mut claim_ids = Vec::new();
        for b in beliefs {
            claim_ids.extend(b.claim_ids.clone());
        }

        Ok(PredictionResult {
            content: response.content,
            confidence,
            method: PredictionMethod::LlmBeliefContext,
            based_on_claims: claim_ids,
            sampling_analysis: None,
        })
    }

    /// Fallback statistical prediction (when LLM is not available)
    async fn predict_statistical_fallback(
        &self,
        beliefs: &[BeliefSummary],
    ) -> crate::Result<PredictionResult> {
        if beliefs.is_empty() {
            return Ok(PredictionResult {
                content: "Insufficient context for prediction.".to_string(),
                confidence: 0.0,
                method: PredictionMethod::StatisticalTrend,
                based_on_claims: vec![],
                sampling_analysis: None,
            });
        }

        // Simple statistical approach: use highest confidence belief
        let best_belief = match beliefs.iter().max_by(|a, b| {
            a.confidence
                .partial_cmp(&b.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            Some(b) => b,
            None => {
                return Ok(PredictionResult {
                    content: "No beliefs available for prediction".to_string(),
                    confidence: 0.0,
                    method: PredictionMethod::StatisticalTrend,
                    based_on_claims: vec![],
                    sampling_analysis: None,
                });
            }
        };

        // Generate a simple prediction based on the best belief
        let confidence_pct = best_belief.confidence * 100.0;
        let prediction = format!(
            "Based on '{}' (confidence: {:.0}%), {}",
            best_belief.content.chars().take(50).collect::<String>(),
            confidence_pct,
            "this belief provides the most reliable basis for decision making."
        );

        Ok(PredictionResult {
            content: prediction,
            confidence: best_belief.confidence * 0.8,
            method: PredictionMethod::StatisticalTrend,
            based_on_claims: best_belief.claim_ids.clone(),
            sampling_analysis: None,
        })
    }

    /// Build context string from beliefs
    fn build_belief_context(&self, beliefs: &[BeliefSummary]) -> String {
        beliefs
            .iter()
            .enumerate()
            .map(|(i, b)| {
                format!(
                    "{}. [{}% confidence] {}",
                    i + 1,
                    (b.confidence * 100.0) as usize,
                    b.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Generate embeddings for the given texts
    pub async fn embed(&self, texts: Vec<String>) -> crate::Result<Vec<Vec<f32>>> {
        self.llm_client
            .embed(texts, Some(self.default_model.clone()))
            .await
    }
}

/// Configuration for prediction service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionConfig {
    /// Model to use for LLM-based predictions
    pub model: String,
    /// Temperature for LLM generation
    pub temperature: f32,
    /// Maximum tokens for prediction response
    pub max_tokens: u32,
    /// Fallback to statistical method if LLM fails
    pub fallback_on_error: bool,
}

impl Default for PredictionConfig {
    fn default() -> Self {
        Self {
            model: "gpt-4".to_string(),
            temperature: 0.3,
            max_tokens: 256,
            fallback_on_error: true,
        }
    }
}

/// Builder for PredictionService
pub struct PredictionServiceBuilder {
    llm_client: Option<Arc<dyn crate::LlmClient>>,
    config: PredictionConfig,
}

impl PredictionServiceBuilder {
    pub fn new() -> Self {
        Self {
            llm_client: None,
            config: PredictionConfig::default(),
        }
    }

    pub fn with_client(mut self, client: Arc<dyn crate::LlmClient>) -> Self {
        self.llm_client = Some(client);
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.config.model = model.into();
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.config.temperature = temperature;
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.config.max_tokens = max_tokens;
        self
    }

    pub fn with_fallback(mut self, fallback: bool) -> Self {
        self.config.fallback_on_error = fallback;
        self
    }

    pub fn build(self) -> Option<PredictionService> {
        // Use placeholder client if none provided, since build_belief_context doesn't need it
        let client = self.llm_client.unwrap_or_else(|| {
            Arc::new(crate::client::PlaceholderClient) as Arc<dyn crate::LlmClient>
        });
        Some(PredictionService {
            llm_client: client,
            default_model: self.config.model,
        })
    }
}

impl Default for PredictionServiceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prediction_config_defaults() {
        let config = PredictionConfig::default();
        assert_eq!(config.model, "gpt-4");
        assert_eq!(config.temperature, 0.3);
        assert_eq!(config.max_tokens, 256);
        assert!(config.fallback_on_error);
    }

    #[test]
    fn test_prediction_service_builder() {
        // Note: This test would need a mock LlmClient in real tests
        let builder = PredictionServiceBuilder::new()
            .with_model("claude-3")
            .with_temperature(0.5)
            .with_max_tokens(512);

        // Without a client, build returns a service with placeholder client
        assert!(builder.build().is_some());
    }

    #[test]
    fn test_build_belief_context() {
        let service = PredictionServiceBuilder::new().build().unwrap();

        let beliefs = vec![
            BeliefSummary {
                claim_id: None,
                content: "Test belief 1".to_string(),
                confidence: 0.8,
                based_on: 3,
                consolidation_stage: cogkos_core::ConsolidationStage::Consolidated,
                claim_ids: vec![],
            },
            BeliefSummary {
                claim_id: None,
                content: "Test belief 2".to_string(),
                confidence: 0.6,
                based_on: 2,
                consolidation_stage: cogkos_core::ConsolidationStage::FastTrack,
                claim_ids: vec![],
            },
        ];

        let context = service.build_belief_context(&beliefs);
        assert!(context.contains("80% confidence"));
        assert!(context.contains("60% confidence"));
    }
}
