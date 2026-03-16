//! MCP Server startup

use std::sync::Arc;

use cogkos_llm::{LlmClient, LlmClientBuilder, PredictionService, ProviderType};
use cogkos_store::Stores;
use rmcp::service::ServiceExt;
use tracing::info;

use super::{CogkosMcpHandler, McpServerState, RateLimiter};
use crate::{AuthMiddleware, McpConfig, QueryCache};

/// Start MCP server with stdio transport
pub async fn start_mcp_server(
    stores: Stores,
    config: McpConfig,
    llm_client: Option<Arc<dyn LlmClient>>,
    embedding_client: Option<Arc<dyn LlmClient>>,
) -> anyhow::Result<()> {
    let auth = Arc::new(AuthMiddleware::new(
        stores.auth.clone(),
        300, // 5 minute cache
    ));

    let cache = Arc::new(QueryCache::new(
        config.cache_max_entries,
        config.cache_ttl_seconds,
    ));

    // Initialize prediction service from environment
    let _prediction_service = if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
        let provider = match std::env::var("LLM_PROVIDER").as_deref() {
            Ok("anthropic") => ProviderType::Anthropic,
            _ => ProviderType::OpenAi,
        };

        let mut builder = LlmClientBuilder::new(api_key, provider);

        if let Ok(base_url) = std::env::var("LLM_BASE_URL") {
            builder = builder.with_base_url(base_url);
        }

        if let Ok(model) = std::env::var("LLM_MODEL") {
            builder = builder.with_model(model);
        }

        let client = builder.build()?;
        let service = PredictionService::new(
            client,
            std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4".to_string()),
        );
        Some(Arc::new(service))
    } else {
        info!("OPENAI_API_KEY not set, prediction service will use statistical fallback");
        None
    };

    let rate_limiter = RateLimiter::new(config.rate_limit_per_minute.unwrap_or(600));

    let state = McpServerState {
        stores,
        auth,
        cache,
        config: config.clone(),
        llm_client,
        embedding_client,
        rate_limiter,
    };

    // Create the server handler
    let handler = CogkosMcpHandler::new(state);

    // Use stdio transport
    let (stdin, stdout) = rmcp::transport::stdio();

    info!("Starting MCP Server with stdio transport");

    // Serve the server
    handler.serve((stdin, stdout)).await?;

    Ok(())
}
