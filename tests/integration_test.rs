//! CogKOS standalone integration tests
//! Run: cargo test --test integration_test

use cogkos_llm::{LlmClientBuilder, ProviderType};

#[tokio::test]
async fn test_llm_client_builder() {
    // Verify builder works without actual API call
    let mut builder = LlmClientBuilder::new("test-key", ProviderType::OpenAi)
        .with_model("gpt-4")
        .with_base_url("http://localhost:8080/v1");
    let client = builder.build().unwrap();

    // Client should be constructable (actual API call requires real key)
    assert!(std::sync::Arc::strong_count(&client) == 1);
}

#[tokio::test]
async fn test_anthropic_client_builder() {
    let mut builder =
        LlmClientBuilder::new("test-key", ProviderType::Anthropic).with_model("claude-sonnet-4-6");
    let client = builder.build().unwrap();
    assert!(std::sync::Arc::strong_count(&client) == 1);
}
