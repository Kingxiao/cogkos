//! MCP Server startup — supports stdio and Streamable HTTP transports

use std::sync::Arc;

use cogkos_llm::{LlmClient, LlmClientBuilder, PredictionService, ProviderType};
use cogkos_store::Stores;
use rmcp::service::ServiceExt;
use tracing::info;

use super::{CogkosMcpHandler, McpServerState, RateLimiter};
use crate::{AuthMiddleware, McpConfig, McpTransport, QueryCache};

/// Build shared MCP server state from config and stores
fn build_state(
    stores: Stores,
    config: &McpConfig,
    llm_client: Option<Arc<dyn LlmClient>>,
    embedding_client: Option<Arc<dyn LlmClient>>,
) -> McpServerState {
    let auth = Arc::new(AuthMiddleware::new(
        stores.auth.clone(),
        config.auth_cache_ttl_seconds,
    ));

    let cache = Arc::new(QueryCache::new(
        config.cache_max_entries,
        config.cache_ttl_seconds,
    ));

    let rate_limit = config.rate_limit_per_minute.unwrap_or(600);
    let rate_limiter = if let Some(ref pool) = config.redis_pool {
        tracing::info!("Rate limiter: Redis-backed (persistent across restarts)");
        RateLimiter::with_redis(pool.clone(), rate_limit)
    } else {
        tracing::info!("Rate limiter: in-memory (resets on restart)");
        RateLimiter::new(rate_limit)
    };

    McpServerState {
        stores,
        auth,
        cache,
        config: config.clone(),
        llm_client,
        embedding_client,
        rate_limiter,
    }
}

/// Initialize optional prediction service from environment
fn _init_prediction_service() -> Option<Arc<PredictionService>> {
    let api_key = std::env::var("OPENAI_API_KEY").ok()?;
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

    let client = builder.build().ok()?;
    let service = PredictionService::new(
        client,
        std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4".to_string()), // verified: 2026-03-21
    );
    Some(Arc::new(service))
}

/// Start MCP server with the configured transport
pub async fn start_mcp_server(
    stores: Stores,
    config: McpConfig,
    llm_client: Option<Arc<dyn LlmClient>>,
    embedding_client: Option<Arc<dyn LlmClient>>,
) -> anyhow::Result<()> {
    match config.transport {
        McpTransport::Stdio => {
            start_stdio_server(stores, config, llm_client, embedding_client).await
        }
        McpTransport::StreamableHttp => {
            start_http_server(stores, config, llm_client, embedding_client).await
        }
    }
}

/// Start MCP server with stdio transport (1:1, single agent)
async fn start_stdio_server(
    stores: Stores,
    config: McpConfig,
    llm_client: Option<Arc<dyn LlmClient>>,
    embedding_client: Option<Arc<dyn LlmClient>>,
) -> anyhow::Result<()> {
    let state = build_state(stores, &config, llm_client, embedding_client);
    let handler = CogkosMcpHandler::new(state);
    let (stdin, stdout) = rmcp::transport::stdio();

    info!("Starting MCP Server with stdio transport");
    handler.serve((stdin, stdout)).await?;
    Ok(())
}

/// Start MCP server with Streamable HTTP transport (1:N, multi-agent)
async fn start_http_server(
    stores: Stores,
    config: McpConfig,
    llm_client: Option<Arc<dyn LlmClient>>,
    embedding_client: Option<Arc<dyn LlmClient>>,
) -> anyhow::Result<()> {
    use axum::Router;
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };
    use tokio_util::sync::CancellationToken;

    let bind_addr = format!("{}:{}", config.host, config.port);
    let cancel_token = CancellationToken::new();

    let state = build_state(stores, &config, llm_client, embedding_client);

    let http_config = StreamableHttpServerConfig {
        stateful_mode: true,
        json_response: false,
        sse_keep_alive: Some(std::time::Duration::from_secs(30)),
        sse_retry: Some(std::time::Duration::from_secs(5)),
        cancellation_token: cancel_token.clone(),
    };

    let session_manager = Arc::new(LocalSessionManager::default());

    // Factory: each session gets its own CogkosMcpHandler instance sharing the same state
    let mcp_service = StreamableHttpService::new(
        move || Ok(CogkosMcpHandler::new(state.clone())),
        session_manager,
        http_config,
    );

    let app = Router::new()
        .route(
            "/mcp",
            axum::routing::any(move |req: axum::extract::Request| {
                let svc = mcp_service.clone();
                async move { svc.handle(req).await }
            }),
        )
        .layer(tower_http::cors::CorsLayer::permissive());

    info!(
        addr = %bind_addr,
        "Starting MCP Server with Streamable HTTP transport on /mcp"
    );

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            cancel_token.cancelled().await;
        })
        .await?;

    Ok(())
}
