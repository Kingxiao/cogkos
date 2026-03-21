//! CogKOS MCP Handler - ServerHandler implementation

use rmcp::{
    ServerHandler,
    model::{
        CallToolResult, Content, ErrorCode, Implementation, InitializeResult, JsonObject,
        ListToolsResult, ServerCapabilities, Tool,
    },
    service::{RequestContext, RoleServer},
};
use std::sync::Arc;
use tracing::info;

use super::McpServerState;
use crate::AuthContext;
use crate::tools::*;

/// CogKOS MCP Handler implementing rmcp's ServerHandler trait
pub struct CogkosMcpHandler {
    state: McpServerState,
    tools: Vec<Tool>,
}

impl CogkosMcpHandler {
    pub fn new(state: McpServerState) -> Self {
        let tools = crate::server::tool_schemas::build_tools();
        Self { state, tools }
    }
}

impl ServerHandler for CogkosMcpHandler {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        // Return ServerInfo directly
        InitializeResult::new(ServerCapabilities::default())
            .with_server_info(Implementation::from_build_env())
    }

    async fn initialize(
        &self,
        _request: rmcp::model::InitializeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, rmcp::ErrorData> {
        Ok(InitializeResult::new(ServerCapabilities::default())
            .with_server_info(Implementation::from_build_env()))
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        Ok(ListToolsResult {
            tools: self.tools.clone(),
            ..Default::default()
        })
    }

    #[tracing::instrument(skip(self, request, context), fields(tool = %request.name))]
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let call_start = std::time::Instant::now();
        let tool_name = request.name.to_string();
        let arguments = request.arguments.unwrap_or_default();

        info!(tool = %tool_name, "Tool call received");

        // Record metrics
        cogkos_core::monitoring::METRICS.inc_counter("cogkos_mcp_tool_calls_total", 1);
        cogkos_core::monitoring::METRICS.inc_counter(&format!("cogkos_mcp_tool_{}", tool_name), 1);

        // Authenticate: tool args > HTTP header > env var
        let auth_context = self
            .get_auth_context_from_args(&arguments, &context)
            .await?;

        // Rate limit per tenant
        self.state
            .rate_limiter
            .check(&auth_context.tenant_id)
            .await?;

        // Input length limits
        const MAX_QUERY_LEN: usize = 10_000;
        const MAX_CONTENT_LEN: usize = 100_000;
        const MAX_UPLOAD_SIZE: usize = 500 * 1024 * 1024; // 500MB

        let timeout_secs = self.state.config.request_timeout_secs;
        let tool_name_for_timeout = tool_name.clone();
        let result = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), async {
            match tool_name.as_str() {
                "query_knowledge" => {
                    if !auth_context.can_read() {
                        return Err(rmcp::ErrorData::new(
                            ErrorCode(-32001),
                            "Permission denied: read access required",
                            None,
                        ));
                    }

                    let req: QueryKnowledgeRequest =
                        serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                            .map_err(|e| {
                                rmcp::ErrorData::new(
                                    ErrorCode::INVALID_PARAMS,
                                    format!("Invalid arguments: {}", e),
                                    None,
                                )
                            })?;

                    if req.query.len() > MAX_QUERY_LEN {
                        return Err(rmcp::ErrorData::new(
                            ErrorCode::INVALID_PARAMS,
                            format!(
                                "Query too long: {} bytes (max {})",
                                req.query.len(),
                                MAX_QUERY_LEN
                            ),
                            None,
                        ));
                    }

                    let response = handle_query_knowledge(
                        req,
                        &auth_context.tenant_id,
                        &[],
                        self.state.stores.claims.as_ref(),
                        self.state.stores.vectors.as_ref(),
                        self.state.stores.graph.as_ref(),
                        self.state.stores.cache.as_ref(),
                        self.state.stores.gaps.as_ref(),
                        self.state.llm_client.clone(),
                        self.state.embedding_client.clone(),
                    )
                    .await
                    .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

                    Ok(CallToolResult::success(vec![Content::text(
                        serde_json::to_string(&response).unwrap_or_default(),
                    )]))
                }
                "submit_experience" => {
                    let req: SubmitExperienceRequest =
                        serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                            .map_err(|e| {
                                rmcp::ErrorData::new(
                                    ErrorCode::INVALID_PARAMS,
                                    format!("Invalid arguments: {}", e),
                                    None,
                                )
                            })?;

                    if req.content.len() > MAX_CONTENT_LEN {
                        return Err(rmcp::ErrorData::new(
                            ErrorCode::INVALID_PARAMS,
                            format!(
                                "Content too long: {} bytes (max {})",
                                req.content.len(),
                                MAX_CONTENT_LEN
                            ),
                            None,
                        ));
                    }

                    if !auth_context.can_write() {
                        return Err(rmcp::ErrorData::new(
                            ErrorCode(-32001),
                            "Permission denied",
                            None,
                        ));
                    }

                    let result = handle_submit_experience(
                        req,
                        &auth_context.tenant_id,
                        Arc::clone(&self.state.stores.claims),
                        Arc::clone(&self.state.stores.vectors),
                        Arc::clone(&self.state.stores.graph),
                        self.state.embedding_client.clone(),
                    )
                    .await
                    .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

                    Ok(CallToolResult::success(vec![Content::text(
                        result.to_string(),
                    )]))
                }
                "submit_feedback" => {
                    if !auth_context.can_write() {
                        return Err(rmcp::ErrorData::new(
                            ErrorCode(-32001),
                            "Permission denied",
                            None,
                        ));
                    }

                    let req: SubmitFeedbackRequest =
                        serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                            .map_err(|e| {
                                rmcp::ErrorData::new(
                                    ErrorCode::INVALID_PARAMS,
                                    format!("Invalid arguments: {}", e),
                                    None,
                                )
                            })?;

                    let result = handle_submit_feedback(
                        req,
                        &auth_context.tenant_id,
                        &auth_context.api_key_hash,
                        self.state.stores.feedback.as_ref(),
                        self.state.stores.cache.as_ref(),
                        self.state.stores.claims.as_ref(),
                    )
                    .await
                    .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

                    Ok(CallToolResult::success(vec![Content::text(
                        result.to_string(),
                    )]))
                }
                "report_gap" => {
                    if !auth_context.can_write() {
                        return Err(rmcp::ErrorData::new(
                            ErrorCode(-32001),
                            "Permission denied",
                            None,
                        ));
                    }

                    let req: ReportGapRequest =
                        serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                            .map_err(|e| {
                                rmcp::ErrorData::new(
                                    ErrorCode::INVALID_PARAMS,
                                    format!("Invalid arguments: {}", e),
                                    None,
                                )
                            })?;

                    let result = handle_report_gap(
                        req,
                        &auth_context.tenant_id,
                        self.state.stores.gaps.as_ref(),
                    )
                    .await
                    .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

                    Ok(CallToolResult::success(vec![Content::text(
                        result.to_string(),
                    )]))
                }
                "get_meta_directory" => {
                    if !auth_context.can_read() {
                        return Err(rmcp::ErrorData::new(
                            ErrorCode(-32001),
                            "Permission denied: read access required",
                            None,
                        ));
                    }

                    let req: GetMetaDirectoryRequest =
                        serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                            .unwrap_or_default();

                    let result = handle_get_meta_directory(
                        req,
                        &auth_context.tenant_id,
                        self.state.stores.claims.as_ref(),
                    )
                    .await
                    .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

                    Ok(CallToolResult::success(vec![Content::text(
                        result.to_string(),
                    )]))
                }
                "upload_document" => {
                    let req: UploadDocumentRequest =
                        serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                            .map_err(|e| {
                                rmcp::ErrorData::new(
                                    ErrorCode::INVALID_PARAMS,
                                    format!("Invalid arguments: {}", e),
                                    None,
                                )
                            })?;

                    // Base64 content is ~4/3 of raw size; check decoded estimate
                    let estimated_size = req.content.len() * 3 / 4;
                    if estimated_size > MAX_UPLOAD_SIZE {
                        return Err(rmcp::ErrorData::new(
                            ErrorCode::INVALID_PARAMS,
                            format!(
                                "File too large: ~{} MB (max {} MB)",
                                estimated_size / 1024 / 1024,
                                MAX_UPLOAD_SIZE / 1024 / 1024
                            ),
                            None,
                        ));
                    }

                    if !auth_context.can_write() {
                        return Err(rmcp::ErrorData::new(
                            ErrorCode(-32001),
                            "Permission denied",
                            None,
                        ));
                    }

                    let embedding_service = self
                        .state
                        .embedding_client
                        .as_ref()
                        .map(|c| cogkos_ingest::EmbeddingService::new(Arc::clone(c)));

                    let result = handle_upload_document(
                        req,
                        &auth_context.tenant_id,
                        self.state.stores.claims.as_ref(),
                        self.state.stores.graph.as_ref(),
                        self.state.stores.vectors.as_ref(),
                        self.state.stores.objects.as_ref(),
                        embedding_service,
                        self.state.llm_client.clone(),
                    )
                    .await
                    .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

                    Ok(CallToolResult::success(vec![Content::text(
                        serde_json::to_string(&result).unwrap_or_default(),
                    )]))
                }
                "subscribe_rss" => {
                    if !auth_context.can_write() {
                        return Err(rmcp::ErrorData::new(
                            ErrorCode(-32001),
                            "Permission denied",
                            None,
                        ));
                    }

                    let req: SubscribeRssRequest =
                        serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                            .map_err(|e| {
                                rmcp::ErrorData::new(
                                    ErrorCode::INVALID_PARAMS,
                                    format!("Invalid arguments: {}", e),
                                    None,
                                )
                            })?;

                    let response = handle_subscribe_rss(
                        req,
                        &auth_context.tenant_id,
                        self.state.stores.subscription.as_ref(),
                    )
                    .await
                    .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

                    Ok(CallToolResult::success(vec![Content::text(
                        serde_json::to_string(&response).unwrap_or_default(),
                    )]))
                }
                "subscribe_webhook" => {
                    if !auth_context.can_write() {
                        return Err(rmcp::ErrorData::new(
                            ErrorCode(-32001),
                            "Permission denied",
                            None,
                        ));
                    }

                    let req: SubscribeWebhookRequest =
                        serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                            .map_err(|e| {
                                rmcp::ErrorData::new(
                                    ErrorCode::INVALID_PARAMS,
                                    format!("Invalid arguments: {}", e),
                                    None,
                                )
                            })?;

                    let response = handle_subscribe_webhook(
                        req,
                        &auth_context.tenant_id,
                        self.state.stores.subscription.as_ref(),
                    )
                    .await
                    .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

                    Ok(CallToolResult::success(vec![Content::text(
                        serde_json::to_string(&response).unwrap_or_default(),
                    )]))
                }
                "subscribe_api" => {
                    if !auth_context.can_write() {
                        return Err(rmcp::ErrorData::new(
                            ErrorCode(-32001),
                            "Permission denied",
                            None,
                        ));
                    }

                    let req: SubscribeApiRequest =
                        serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                            .map_err(|e| {
                                rmcp::ErrorData::new(
                                    ErrorCode::INVALID_PARAMS,
                                    format!("Invalid arguments: {}", e),
                                    None,
                                )
                            })?;

                    let response = handle_subscribe_api(
                        req,
                        &auth_context.tenant_id,
                        self.state.stores.subscription.as_ref(),
                    )
                    .await
                    .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

                    Ok(CallToolResult::success(vec![Content::text(
                        serde_json::to_string(&response).unwrap_or_default(),
                    )]))
                }
                "list_subscriptions" => {
                    if !auth_context.can_read() {
                        return Err(rmcp::ErrorData::new(
                            ErrorCode(-32001),
                            "Permission denied: read access required",
                            None,
                        ));
                    }

                    let req: ListSubscriptionsRequest =
                        serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                            .map_err(|e| {
                                rmcp::ErrorData::new(
                                    ErrorCode::INVALID_PARAMS,
                                    format!("Invalid arguments: {}", e),
                                    None,
                                )
                            })?;

                    let response = handle_list_subscriptions(
                        req,
                        &auth_context.tenant_id,
                        self.state.stores.subscription.as_ref(),
                    )
                    .await
                    .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

                    Ok(CallToolResult::success(vec![Content::text(
                        serde_json::to_string(&response).unwrap_or_default(),
                    )]))
                }
                _ => Err(rmcp::ErrorData::new(
                    ErrorCode::METHOD_NOT_FOUND,
                    tool_name,
                    None,
                )),
            }
        })
        .await
        .unwrap_or_else(|_| {
            tracing::error!(tool = %tool_name_for_timeout, timeout_secs, "Tool call timed out");
            Err(rmcp::ErrorData::new(
                ErrorCode(-32002),
                format!("Tool call timed out after {}s", timeout_secs),
                None,
            ))
        });

        cogkos_core::monitoring::METRICS
            .record_duration("cogkos_mcp_call_duration_seconds", call_start.elapsed());
        cogkos_core::monitoring::METRICS.inc_counter("cogkos_mcp_calls_total", 1);

        result
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|t| t.name.as_ref() == name).cloned()
    }
}

impl CogkosMcpHandler {
    /// Extract and validate authentication from tool arguments, HTTP headers, or env var.
    ///
    /// Authentication priority:
    /// 1. `api_key` field in tool call arguments (stdio transport)
    /// 2. `x-api-key` HTTP header (Streamable HTTP transport)
    /// 3. `DEFAULT_MCP_API_KEY` env var (dev mode — bypasses DB, see auth.rs)
    async fn get_auth_context_from_args(
        &self,
        arguments: &JsonObject,
        context: &RequestContext<RoleServer>,
    ) -> Result<AuthContext, rmcp::ErrorData> {
        let api_key = if let Some(key) = arguments.get("api_key").and_then(|v| v.as_str()) {
            key.to_string()
        } else if let Some(key) = Self::extract_api_key_from_headers(context) {
            key
        } else if let Ok(default_key) = std::env::var("DEFAULT_MCP_API_KEY") {
            default_key
        } else {
            return Err(rmcp::ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                "Missing api_key: provide in arguments, X-API-Key header, or set DEFAULT_MCP_API_KEY",
                None,
            ));
        };

        self.state.auth.authenticate(&api_key).await.map_err(|e| {
            rmcp::ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                format!("Authentication failed: {}", e),
                None,
            )
        })
    }

    /// Try to extract API key from HTTP request headers via rmcp's RequestContext.
    fn extract_api_key_from_headers(context: &RequestContext<RoleServer>) -> Option<String> {
        let parts = context.extensions.get::<http::request::Parts>()?;
        let value = parts.headers.get("x-api-key")?;
        value.to_str().ok().map(|s| s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_tools_returns_all_tools() {
        let tools = crate::server::tool_schemas::build_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"query_knowledge"));
        assert!(names.contains(&"submit_experience"));
        assert!(names.contains(&"submit_feedback"));
        assert!(names.contains(&"report_gap"));
        assert!(names.contains(&"get_meta_directory"));
        assert!(names.contains(&"upload_document"));
        assert!(names.contains(&"subscribe_rss"));
        assert!(names.contains(&"subscribe_webhook"));
        assert!(names.contains(&"subscribe_api"));
        assert!(names.contains(&"list_subscriptions"));
        assert_eq!(tools.len(), 10);
    }

    #[test]
    fn build_tools_all_have_api_key() {
        let tools = crate::server::tool_schemas::build_tools();
        for tool in &tools {
            let schema = &tool.input_schema;
            assert!(
                schema.contains_key("api_key"),
                "Tool {} missing api_key in schema",
                tool.name
            );
        }
    }
}
