//! MCP tool schema definitions

use rmcp::model::{JsonObject, Tool};

/// Inject api_key as optional field into every tool schema.
/// When using Streamable HTTP transport, clients can pass api_key via
/// the `X-API-Key` HTTP header instead of tool arguments.
fn inject_api_key(schema: &mut JsonObject) {
    schema.insert(
        "api_key".to_string(),
        serde_json::json!({"type": "string", "description": "API key for authentication. Optional when X-API-Key header is provided."}),
    );
    // api_key is intentionally NOT added to required — header auth is the primary path
}

/// Build all MCP tool schema definitions
pub fn build_tools() -> Vec<Tool> {
    let mut tools = Vec::new();

    // query_knowledge
    let mut input_schema = JsonObject::new();
    input_schema.insert("query".to_string(), serde_json::json!({"type": "string"}));
    input_schema.insert("context".to_string(), serde_json::json!({"type": "object"}));
    input_schema.insert(
        "include_predictions".to_string(),
        serde_json::json!({"type": "boolean"}),
    );
    input_schema.insert(
        "include_conflicts".to_string(),
        serde_json::json!({"type": "boolean"}),
    );
    input_schema.insert(
        "include_gaps".to_string(),
        serde_json::json!({"type": "boolean"}),
    );
    input_schema.insert(
        "memory_layer".to_string(),
        serde_json::json!({"type": "string", "enum": ["working", "episodic", "semantic"], "description": "Filter by memory layer"}),
    );
    input_schema.insert(
        "session_id".to_string(),
        serde_json::json!({"type": "string", "description": "Filter by session ID"}),
    );
    input_schema.insert(
        "agent_id".to_string(),
        serde_json::json!({"type": "string", "description": "Agent ID for episodic memory scoping — only returns this agent's experiences"}),
    );
    input_schema.insert(
        "namespace".to_string(),
        serde_json::json!({"type": "string", "description": "Namespace filter for intra-tenant isolation. Claims without namespace are always visible."}),
    );
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
    input_schema.insert(
        "node_type".to_string(),
        serde_json::json!({"type": "string"}),
    );
    input_schema.insert(
        "confidence".to_string(),
        serde_json::json!({"type": "number"}),
    );
    input_schema.insert("source".to_string(), serde_json::json!({
        "type": "object",
        "description": "Source info. For human: {\"type\": \"human\", \"user_id\": \"...\", \"role\": \"user\"} (role defaults to \"user\")"
    }));
    input_schema.insert("tags".to_string(), serde_json::json!({"type": "array"}));
    input_schema.insert(
        "memory_layer".to_string(),
        serde_json::json!({"type": "string", "enum": ["working", "episodic", "semantic"], "description": "Memory layer (default: semantic)"}),
    );
    input_schema.insert(
        "session_id".to_string(),
        serde_json::json!({"type": "string", "description": "Session ID for working/episodic memory scoping"}),
    );
    input_schema.insert(
        "namespace".to_string(),
        serde_json::json!({"type": "string", "description": "Namespace for intra-tenant isolation (e.g. client project scoping)"}),
    );
    input_schema.insert(
        "required".to_string(),
        serde_json::json!(["content", "node_type", "source"]),
    );
    inject_api_key(&mut input_schema);
    tools.push(Tool::new(
        "submit_experience",
        "Submit experience/observation to the knowledge base",
        input_schema,
    ));

    // submit_feedback
    let mut input_schema = JsonObject::new();
    input_schema.insert(
        "query_hash".to_string(),
        serde_json::json!({"type": "integer"}),
    );
    input_schema.insert(
        "success".to_string(),
        serde_json::json!({"type": "boolean"}),
    );
    input_schema.insert("note".to_string(), serde_json::json!({"type": "string"}));
    input_schema.insert(
        "agent_id".to_string(),
        serde_json::json!({"type": "string", "description": "Agent identity for feedback attribution. Defaults to {tenant_id}/anonymous."}),
    );
    input_schema.insert(
        "required".to_string(),
        serde_json::json!(["query_hash", "success"]),
    );
    inject_api_key(&mut input_schema);
    tools.push(Tool::new(
        "submit_feedback",
        "Submit feedback on previous query results",
        input_schema,
    ));

    // report_gap
    let mut input_schema = JsonObject::new();
    input_schema.insert("domain".to_string(), serde_json::json!({"type": "string"}));
    input_schema.insert(
        "description".to_string(),
        serde_json::json!({"type": "string"}),
    );
    input_schema.insert(
        "priority".to_string(),
        serde_json::json!({"type": "string"}),
    );
    input_schema.insert(
        "required".to_string(),
        serde_json::json!(["domain", "description"]),
    );
    inject_api_key(&mut input_schema);
    tools.push(Tool::new(
        "report_gap",
        "Report a knowledge gap",
        input_schema,
    ));

    // get_meta_directory
    let mut input_schema = JsonObject::new();
    input_schema.insert(
        "query_domain".to_string(),
        serde_json::json!({"type": "string"}),
    );
    inject_api_key(&mut input_schema);
    tools.push(Tool::new(
        "get_meta_directory",
        "Get meta knowledge directory",
        input_schema,
    ));

    // upload_document
    let mut input_schema = JsonObject::new();
    input_schema.insert(
        "filename".to_string(),
        serde_json::json!({"type": "string"}),
    );
    input_schema.insert(
        "content_base64".to_string(),
        serde_json::json!({"type": "string"}),
    );
    input_schema.insert("source".to_string(), serde_json::json!({
        "type": "object",
        "description": "Source info. For human: {\"type\": \"human\", \"user_id\": \"...\", \"role\": \"user\"} (role defaults to \"user\")"
    }));
    input_schema.insert("tags".to_string(), serde_json::json!({"type": "array"}));
    input_schema.insert(
        "auto_process".to_string(),
        serde_json::json!({"type": "boolean"}),
    );
    input_schema.insert(
        "namespace".to_string(),
        serde_json::json!({"type": "string", "description": "Namespace for intra-tenant isolation"}),
    );
    input_schema.insert(
        "required".to_string(),
        serde_json::json!(["filename", "content_base64", "source"]),
    );
    inject_api_key(&mut input_schema);
    tools.push(Tool::new(
        "upload_document",
        "Upload document to CogKOS for ingestion",
        input_schema,
    ));

    // subscribe_rss
    let mut input_schema = JsonObject::new();
    input_schema.insert(
        "url".to_string(),
        serde_json::json!({"type": "string", "description": "RSS feed URL"}),
    );
    input_schema.insert(
        "poll_interval_secs".to_string(),
        serde_json::json!({"type": "number", "description": "Polling interval in seconds"}),
    );
    input_schema.insert(
        "max_items".to_string(),
        serde_json::json!({"type": "number", "description": "Maximum items per poll"}),
    );
    input_schema.insert(
        "fetch_full_content".to_string(),
        serde_json::json!({"type": "boolean", "description": "Whether to fetch full content"}),
    );
    input_schema.insert("required".to_string(), serde_json::json!(["url"]));
    inject_api_key(&mut input_schema);
    tools.push(Tool::new(
        "subscribe_rss",
        "Subscribe to an RSS feed for continuous knowledge ingestion",
        input_schema,
    ));

    // subscribe_webhook
    let mut input_schema = JsonObject::new();
    input_schema.insert(
        "url".to_string(),
        serde_json::json!({"type": "string", "description": "Webhook endpoint URL"}),
    );
    input_schema.insert(
        "secret".to_string(),
        serde_json::json!({"type": "string", "description": "Secret for signature validation"}),
    );
    input_schema.insert(
        "events".to_string(),
        serde_json::json!({"type": "array", "description": "Event types to subscribe to"}),
    );
    input_schema.insert("required".to_string(), serde_json::json!(["url"]));
    inject_api_key(&mut input_schema);
    tools.push(Tool::new(
        "subscribe_webhook",
        "Register a webhook endpoint for receiving external knowledge updates",
        input_schema,
    ));

    // subscribe_api
    let mut input_schema = JsonObject::new();
    input_schema.insert(
        "url".to_string(),
        serde_json::json!({"type": "string", "description": "API endpoint URL"}),
    );
    input_schema.insert(
        "poll_interval_secs".to_string(),
        serde_json::json!({"type": "number", "description": "Polling interval in seconds"}),
    );
    input_schema.insert(
        "method".to_string(),
        serde_json::json!({"type": "string", "description": "HTTP method"}),
    );
    input_schema.insert(
        "headers".to_string(),
        serde_json::json!({"type": "object", "description": "Request headers"}),
    );
    input_schema.insert(
        "body".to_string(),
        serde_json::json!({"type": "string", "description": "Request body for POST"}),
    );
    input_schema.insert("required".to_string(), serde_json::json!(["url"]));
    inject_api_key(&mut input_schema);
    tools.push(Tool::new(
        "subscribe_api",
        "Subscribe to an API endpoint for periodic polling",
        input_schema,
    ));

    // manage_claim
    let mut input_schema = JsonObject::new();
    input_schema.insert(
        "claim_id".to_string(),
        serde_json::json!({"type": "string", "description": "UUID of the claim to manage"}),
    );
    input_schema.insert(
        "action".to_string(),
        serde_json::json!({
            "type": "object",
            "description": "Action to perform: {\"type\": \"promote\", \"knowledge_type\": \"Business\"}, {\"type\": \"demote\", \"knowledge_type\": \"Experiential\"}, {\"type\": \"set_confidence\", \"confidence\": 0.8}, or {\"type\": \"retract\", \"reason\": \"...\"}"
        }),
    );
    input_schema.insert(
        "required".to_string(),
        serde_json::json!(["claim_id", "action"]),
    );
    inject_api_key(&mut input_schema);
    tools.push(Tool::new(
        "manage_claim",
        "Manage a claim: promote, demote, set confidence, or retract",
        input_schema,
    ));

    // batch_invalidate
    let mut input_schema = JsonObject::new();
    input_schema.insert(
        "domain".to_string(),
        serde_json::json!({"type": "string", "description": "Filter by domain"}),
    );
    input_schema.insert(
        "tags".to_string(),
        serde_json::json!({"type": "array", "items": {"type": "string"}, "description": "Filter by tags"}),
    );
    input_schema.insert(
        "created_before".to_string(),
        serde_json::json!({"type": "string", "format": "date-time", "description": "Filter claims created before this timestamp"}),
    );
    input_schema.insert(
        "knowledge_type".to_string(),
        serde_json::json!({"type": "string", "enum": ["Business", "Experiential"], "description": "Filter by knowledge type"}),
    );
    inject_api_key(&mut input_schema);
    tools.push(Tool::new(
        "batch_invalidate",
        "Batch invalidate (retract) claims matching filter criteria",
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
