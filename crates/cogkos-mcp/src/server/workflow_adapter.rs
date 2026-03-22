//! Adapts cogkos_llm::LlmClient to workflow planner's PlannerLlmClient trait.

use std::sync::Arc;

use async_trait::async_trait;
use cogkos_llm::{LlmClient, LlmRequest, Message, Role};
use cogkos_workflow::PlannerLlmClient;

/// Bridges the full LlmClient to the minimal PlannerLlmClient interface.
pub struct LlmPlannerAdapter {
    client: Arc<dyn LlmClient>,
}

impl LlmPlannerAdapter {
    pub fn new(client: Arc<dyn LlmClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl PlannerLlmClient for LlmPlannerAdapter {
    async fn generate(&self, prompt: &str) -> Result<String, String> {
        let request = LlmRequest {
            messages: vec![Message {
                role: Role::User,
                content: prompt.to_string(),
            }],
            temperature: 0.3,
            max_tokens: Some(1024),
            ..Default::default()
        };
        self.client
            .chat(request)
            .await
            .map(|r| r.content)
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_wraps_client() {
        let client = Arc::new(cogkos_llm::client::PlaceholderClient);
        let adapter = LlmPlannerAdapter::new(client);
        let _: &dyn PlannerLlmClient = &adapter;
    }
}
