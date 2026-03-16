//! CogKOS MCP Handler - ServerHandler implementation

use rmcp::{
    model::{
        CallToolResult, Content, ErrorCode, Implementation, InitializeResult, JsonObject,
        ListToolsResult, ServerCapabilities, Tool,
    },
    service::{RequestContext, RoleServer},
    ServerHandler,
};
use tracing::info;

use crate::tools::*;
use crate::AuthContext;
use super::McpServerState;

/// CogKOS MCP Handler implementing rmcp's ServerHandler trait
pub struct CogkosMcpHandler {
    state: McpServerState,
    tools: Vec<Tool>,
}

impl CogkosMcpHandler {
    pub fn new(state: McpServerState) -> Self {
        let tools = Self::build_tools();
        Self { state, tools }
    }

    fn build_tools() -> Vec<Tool> {
        let mut tools = Vec::new();

        // Helper: inject api_key into every tool schema and required array
        let inject_api_key = |schema: &mut JsonObject| {
            schema.insert(
                "api_key".to_string(),
                serde_json::json!({"type": "string", "description": "API key for authentication"}),
            );
            if let Some(serde_json::Value::Array(required)) = schema.get_mut("required") {
                required.push(serde_json::json!("api_key"));
            } else {
                schema.insert("required".to_string(), serde_json::json!(["api_key"]));
            }
        };

        // query_knowledge
        let mut input_schema = JsonObject::new();
        input_schema.insert("query".to_string(), serde_json::json!({"type": "string"}));
        input_schema.insert("context".to_string(), serde_json::json!({"type": "object"}));
        input_schema.insert("include_predictions".to_string(), serde_json::json!({"type": "boolean"}));
        input_schema.insert("include_conflicts".to_string(), serde_json::json!({"type": "boolean"}));
        input_schema.insert("include_gaps".to_string(), serde_json::json!({"type": "boolean"}));
        input_schema.insert("required".to_string(), serde_json::json!(["query"]));
        inject_api_key(&mut input_schema);
        tools.push(Tool::new(
            "query_knowledge",
            "Query the knowledge base for decision support",
            input_schema,
        ));

        // submit_experience
        let mut input_schema = JsonObject::new();
        input_schema.insert("content".to_string(), serde_json::json!({"type": "string"}));
        input_schema.insert("node_type".to_string(), serde_json::json!({"type": "string"}));
        input_schema.insert("confidence".to_string(), serde_json::json!({"type": "number"}));
        input_schema.insert("source".to_string(), serde_json::json!({"type": "object"}));
        input_schema.insert("tags".to_string(), serde_json::json!({"type": "array"}));
        input_schema.insert("required".to_string(), serde_json::json!(["content", "node_type", "source"]));
        inject_api_key(&mut input_schema);
        tools.push(Tool::new(
            "submit_experience",
            "Submit experience/observation to the knowledge base",
            input_schema,
        ));

        // submit_feedback
        let mut input_schema = JsonObject::new();
        input_schema.insert("query_hash".to_string(), serde_json::json!({"type": "integer"}));
        input_schema.insert("success".to_string(), serde_json::json!({"type": "boolean"}));
        input_schema.insert("note".to_string(), serde_json::json!({"type": "string"}));
        input_schema.insert("required".to_string(), serde_json::json!(["query_hash", "success"]));
        inject_api_key(&mut input_schema);
        tools.push(Tool::new(
            "submit_feedback",
            "Submit feedback on previous query results",
            input_schema,
        ));

        // report_gap
        let mut input_schema = JsonObject::new();
        input_schema.insert("domain".to_string(), serde_json::json!({"type": "string"}));
        input_schema.insert("description".to_string(), serde_json::json!({"type": "string"}));
        input_schema.insert("priority".to_string(), serde_json::json!({"type": "string"}));
        input_schema.insert("required".to_string(), serde_json::json!(["domain", "description"]));
        inject_api_key(&mut input_schema);
        tools.push(Tool::new(
            "report_gap",
            "Report a knowledge gap",
            input_schema,
        ));

        // get_meta_directory
        let mut input_schema = JsonObject::new();
        input_schema.insert("query_domain".to_string(), serde_json::json!({"type": "string"}));
        inject_api_key(&mut input_schema);
        tools.push(Tool::new(
            "get_meta_directory",
            "Get meta knowledge directory",
            input_schema,
        ));

        // upload_document
        let mut input_schema = JsonObject::new();
        input_schema.insert("filename".to_string(), serde_json::json!({"type": "string"}));
        input_schema.insert("content_base64".to_string(), serde_json::json!({"type": "string"}));
        input_schema.insert("source".to_string(), serde_json::json!({"type": "object"}));
        input_schema.insert("tags".to_string(), serde_json::json!({"type": "array"}));
        input_schema.insert("auto_process".to_string(), serde_json::json!({"type": "boolean"}));
        input_schema.insert("required".to_string(), serde_json::json!(["filename", "content_base64", "source"]));
        inject_api_key(&mut input_schema);
        tools.push(Tool::new(
            "upload_document",
            "Upload document to CogKOS for ingestion",
            input_schema,
        ));

        // subscribe_rss
        let mut input_schema = JsonObject::new();
        input_schema.insert("url".to_string(), serde_json::json!({"type": "string", "description": "RSS feed URL"}));
        input_schema.insert("poll_interval_secs".to_string(), serde_json::json!({"type": "number", "description": "Polling interval in seconds"}));
        input_schema.insert("max_items".to_string(), serde_json::json!({"type": "number", "description": "Maximum items per poll"}));
        input_schema.insert("fetch_full_content".to_string(), serde_json::json!({"type": "boolean", "description": "Whether to fetch full content"}));
        input_schema.insert("required".to_string(), serde_json::json!(["url"]));
        inject_api_key(&mut input_schema);
        tools.push(Tool::new(
            "subscribe_rss",
            "Subscribe to an RSS feed for continuous knowledge ingestion",
            input_schema,
        ));

        // subscribe_webhook
        let mut input_schema = JsonObject::new();
        input_schema.insert("url".to_string(), serde_json::json!({"type": "string", "description": "Webhook endpoint URL"}));
        input_schema.insert("secret".to_string(), serde_json::json!({"type": "string", "description": "Secret for signature validation"}));
        input_schema.insert("events".to_string(), serde_json::json!({"type": "array", "description": "Event types to subscribe to"}));
        input_schema.insert("required".to_string(), serde_json::json!(["url"]));
        inject_api_key(&mut input_schema);
        tools.push(Tool::new(
            "subscribe_webhook",
            "Register a webhook endpoint for receiving external knowledge updates",
            input_schema,
        ));

        // subscribe_api
        let mut input_schema = JsonObject::new();
        input_schema.insert("url".to_string(), serde_json::json!({"type": "string", "description": "API endpoint URL"}));
        input_schema.insert("poll_interval_secs".to_string(), serde_json::json!({"type": "number", "description": "Polling interval in seconds"}));
        input_schema.insert("method".to_string(), serde_json::json!({"type": "string", "description": "HTTP method"}));
        input_schema.insert("headers".to_string(), serde_json::json!({"type": "object", "description": "Request headers"}));
        input_schema.insert("body".to_string(), serde_json::json!({"type": "string", "description": "Request body for POST"}));
        input_schema.insert("required".to_string(), serde_json::json!(["url"]));
        inject_api_key(&mut input_schema);
        tools.push(Tool::new(
            "subscribe_api",
            "Subscribe to an API endpoint for periodic polling",
            input_schema,
        ));

        // list_subscriptions
        let mut input_schema = JsonObject::new();
        input_schema.insert("type".to_string(), serde_json::json!({"type": "string", "enum": ["rss", "webhook", "api"], "description": "Subscription type"}));
        input_schema.insert("required".to_string(), serde_json::json!(["type"]));
        inject_api_key(&mut input_schema);
        tools.push(Tool::new(
            "list_subscriptions",
            "List active knowledge subscriptions",
            input_schema,
        ));

        tools
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

    #[tracing::instrument(skip(self, request, _context), fields(tool = %request.name))]
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let call_start = std::time::Instant::now();
        let tool_name = request.name.to_string();
        let arguments = request.arguments.unwrap_or_default();

        info!(tool = %tool_name, "Tool call received");

        // Record metrics
        cogkos_core::monitoring::METRICS.inc_counter("cogkos_mcp_tool_calls_total", 1);
        cogkos_core::monitoring::METRICS.inc_counter(&format!("cogkos_mcp_tool_{}", tool_name), 1);

        // Authenticate via api_key in arguments
        let auth_context = self.get_auth_context_from_args(&arguments).await?;

        // Rate limit per tenant
        self.state.rate_limiter.check(&auth_context.tenant_id).await?;

        // Input length limits
        const MAX_QUERY_LEN: usize = 10_000;
        const MAX_CONTENT_LEN: usize = 100_000;
        const MAX_UPLOAD_SIZE: usize = 500 * 1024 * 1024; // 500MB

        let result = match tool_name.as_str() {
            "query_knowledge" => {
                if !auth_context.can_read() {
                    return Err(rmcp::ErrorData::new(
                        ErrorCode(-32001),
                        "Permission denied: read access required",
                        None,
                    ));
                }

                let req: QueryKnowledgeRequest = serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                    .map_err(|e| rmcp::ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        format!("Invalid arguments: {}", e),
                        None,
                    ))?;

                if req.query.len() > MAX_QUERY_LEN {
                    return Err(rmcp::ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        format!("Query too long: {} bytes (max {})", req.query.len(), MAX_QUERY_LEN),
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
                let req: SubmitExperienceRequest = serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                    .map_err(|e| rmcp::ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        format!("Invalid arguments: {}", e),
                        None,
                    ))?;

                if req.content.len() > MAX_CONTENT_LEN {
                    return Err(rmcp::ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        format!("Content too long: {} bytes (max {})", req.content.len(), MAX_CONTENT_LEN),
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
                    self.state.stores.claims.as_ref(),
                    self.state.stores.vectors.as_ref(),
                    self.state.stores.graph.as_ref(),
                    self.state.embedding_client.clone(),
                )
                .await
                .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

                Ok(CallToolResult::success(vec![Content::text(result.to_string())]))
            }
            "submit_feedback" => {
                if !auth_context.can_write() {
                    return Err(rmcp::ErrorData::new(
                        ErrorCode(-32001),
                        "Permission denied",
                        None,
                    ));
                }

                let req: SubmitFeedbackRequest = serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                    .map_err(|e| rmcp::ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        format!("Invalid arguments: {}", e),
                        None,
                    ))?;

                let result = handle_submit_feedback(
                    req,
                    &auth_context.tenant_id,
                    &auth_context.api_key_hash,
                    self.state.stores.feedback.as_ref(),
                    self.state.stores.cache.as_ref(),
                )
                .await
                .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

                Ok(CallToolResult::success(vec![Content::text(result.to_string())]))
            }
            "report_gap" => {
                if !auth_context.can_write() {
                    return Err(rmcp::ErrorData::new(
                        ErrorCode(-32001),
                        "Permission denied",
                        None,
                    ));
                }

                let req: ReportGapRequest = serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                    .map_err(|e| rmcp::ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        format!("Invalid arguments: {}", e),
                        None,
                    ))?;

                let result = handle_report_gap(req, &auth_context.tenant_id, self.state.stores.gaps.as_ref())
                    .await
                    .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

                Ok(CallToolResult::success(vec![Content::text(result.to_string())]))
            }
            "get_meta_directory" => {
                if !auth_context.can_read() {
                    return Err(rmcp::ErrorData::new(
                        ErrorCode(-32001),
                        "Permission denied: read access required",
                        None,
                    ));
                }

                let req: GetMetaDirectoryRequest = serde_json::from_value(serde_json::Value::Object(arguments.clone())).unwrap_or_default();

                let result = handle_get_meta_directory(req, &auth_context.tenant_id, self.state.stores.claims.as_ref())
                    .await
                    .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

                Ok(CallToolResult::success(vec![Content::text(result.to_string())]))
            }
            "upload_document" => {
                let req: UploadDocumentRequest = serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                    .map_err(|e| rmcp::ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        format!("Invalid arguments: {}", e),
                        None,
                    ))?;

                // Base64 content is ~4/3 of raw size; check decoded estimate
                let estimated_size = req.content.len() * 3 / 4;
                if estimated_size > MAX_UPLOAD_SIZE {
                    return Err(rmcp::ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        format!("File too large: ~{} MB (max {} MB)", estimated_size / 1024 / 1024, MAX_UPLOAD_SIZE / 1024 / 1024),
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

                let result = handle_upload_document(
                    req,
                    &auth_context.tenant_id,
                    self.state.stores.claims.as_ref(),
                    self.state.stores.graph.as_ref(),
                    self.state.stores.vectors.as_ref(),
                    self.state.stores.objects.as_ref(),
                    None,
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

                let req: SubscribeRssRequest = serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                    .map_err(|e| rmcp::ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        format!("Invalid arguments: {}", e),
                        None,
                    ))?;

                let response = handle_subscribe_rss(req, &auth_context.tenant_id, self.state.stores.subscription.as_ref())
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

                let req: SubscribeWebhookRequest = serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                    .map_err(|e| rmcp::ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        format!("Invalid arguments: {}", e),
                        None,
                    ))?;

                let response = handle_subscribe_webhook(req, &auth_context.tenant_id, self.state.stores.subscription.as_ref())
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

                let req: SubscribeApiRequest = serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                    .map_err(|e| rmcp::ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        format!("Invalid arguments: {}", e),
                        None,
                    ))?;

                let response = handle_subscribe_api(req, &auth_context.tenant_id, self.state.stores.subscription.as_ref())
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

                let req: ListSubscriptionsRequest = serde_json::from_value(serde_json::Value::Object(arguments.clone()))
                    .map_err(|e| rmcp::ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        format!("Invalid arguments: {}", e),
                        None,
                    ))?;

                let response = handle_list_subscriptions(req, &auth_context.tenant_id, self.state.stores.subscription.as_ref())
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
        };

        cogkos_core::monitoring::METRICS.record_duration("cogkos_mcp_call_duration_seconds", call_start.elapsed());
        cogkos_core::monitoring::METRICS.inc_counter("cogkos_mcp_calls_total", 1);

        result
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|t| t.name.as_ref() == name).cloned()
    }
}

impl CogkosMcpHandler {
    /// Extract and validate authentication from tool arguments.
    ///
    /// MCP stdio transport has no HTTP headers, so the API key is passed
    /// as an `api_key` field in the tool call arguments. The key is validated
    /// against the AuthStore to resolve tenant_id and permissions.
    async fn get_auth_context_from_args(
        &self,
        arguments: &JsonObject,
    ) -> Result<AuthContext, rmcp::ErrorData> {
        let api_key = arguments
            .get("api_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                rmcp::ErrorData::new(
                    ErrorCode::INVALID_PARAMS,
                    "Missing required field: api_key",
                    None,
                )
            })?;

        self.state.auth.authenticate(api_key).await.map_err(|e| {
            rmcp::ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                format!("Authentication failed: {}", e),
                None,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_tools_returns_all_tools() {
        let tools = CogkosMcpHandler::build_tools();
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
        let tools = CogkosMcpHandler::build_tools();
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
