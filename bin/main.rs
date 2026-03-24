use cogkos_llm::LlmClientBuilder;
use cogkos_llm::LlmConfig;
use cogkos_llm::ProviderType;
use cogkos_mcp::{start_mcp_server, McpConfig};
use cogkos_store::{Stores, InMemoryGraphStore, InMemoryCacheStore, InMemoryAuthStoreWithKey, InMemoryGapStore, AuthStore, InMemorySubscriptionStore, PostgresStore, InMemoryClaimStore, InMemoryFeedbackStore};
use cogkos_core::audit::InMemoryAuditStore;
use cogkos_store::vector::InMemoryVectorStore;
use cogkos_store::s3::LocalStore;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    println!("CogKOS v0.1.0 - Cognitive Knowledge OS");
    println!("============================");
    println!();

    println!("Available modules:");
    println!("  - cogkos-core: Core functionality");
    println!("  - cogkos-ingest: File ingestion");
    println!("  - cogkos-llm: LLM integration");
    println!("  - cogkos-store: Storage layer");
    println!();

    // Load LLM config
    let llm_config = load_llm_config();
    println!("LLM config: {}", llm_config);

    // Create LLM client
    let llm_client: Option<Arc<dyn cogkos_llm::LlmClient>> = if llm_config.is_configured("text") {
        let config = llm_config.get("text").unwrap();
        let mut builder = LlmClientBuilder::new(&config.api_key, ProviderType::OpenAi)
            .with_base_url(config.base_url.as_deref().unwrap_or("https://api.moonshot.cn/v1")) // verified: 2026-03-21
            .with_model(&config.model);
        match builder.build()
        {
            Ok(client) => {
                println!("  ✓ Text LLM: {} ({})", config.model, config.provider);
                Some(client)
            }
            Err(e) => {
                println!("  ⚠ Text LLM build failed: {}", e);
                None
            }
        }
    } else {
        println!("  ⚠ Text LLM not configured (needs KIMI_API_KEY)");
        None
    };

    // Check other LLM configs
    if llm_config.is_configured("embedding") {
        let config = llm_config.get("embedding").unwrap();
        println!("  ✓ Embedding: {} ({})", config.model, config.provider);
    } else {
        println!("  ⚠ Embedding not configured (needs AI302_API_KEY)");
    }

    if llm_config.is_configured("image") {
        let config = llm_config.get("image").unwrap();
        println!("  ✓ Image: {} ({})", config.model, config.provider);
    } else {
        println!("  ⚠ Image not configured (needs DOUBAO_API_KEY)");
    }

    if llm_config.is_configured("audio") {
        let config = llm_config.get("audio").unwrap();
        println!("  ✓ Audio: {} ({})", config.model, config.provider);
    } else {
        println!("  ⚠ Audio not configured (needs OPENAI_API_KEY)");
    }

    if llm_config.is_configured("other") {
        let config = llm_config.get("other").unwrap();
        println!("  ✓ Other: {} ({})", config.model, config.provider);
    } else {
        println!("  ⚠ Other not configured (needs OPENROUTER_API_KEY)");
    }

    println!();
    println!("Environment config:");
    println!("  DATABASE_URL: {}", env::var("DATABASE_URL").unwrap_or_else(|_| "✗ not configured".to_string()));
    println!("  QDRANT_URL: {}", env::var("QDRANT_URL").unwrap_or_else(|_| "✗ not configured".to_string()));
    println!("  FALKORDB_URL: {}", env::var("FALKORDB_URL").unwrap_or_else(|_| "✗ not configured".to_string()));
    println!();

    // Create Stores - use fixed test key
    let auth_store = InMemoryAuthStoreWithKey::new();
    let test_key = auth_store.create_api_key("test-tenant", vec!["read".to_string(), "write".to_string()]).await.unwrap();
    println!("  ✓ Test API key: {}\n", test_key);

    let object_store: Arc<dyn cogkos_store::ObjectStore> = match LocalStore::new("cogkos").await {
        Ok(store) => Arc::new(store),
        Err(e) => {
            println!("❌ Object store init failed: {:?}", e);
            return;
        }
    };

    // Use PostgreSQL for persistence if DATABASE_URL is set
    let db_url = std::env::var("DATABASE_URL").unwrap_or_default();
    let stores = if !db_url.is_empty() {
        println!("🔄 Connecting to PostgreSQL...");
        let postgres_store = cogkos_store::PostgresStore::from_url(&db_url)
            .await
            .expect("Failed to connect to PostgreSQL");
        println!("✅ PostgreSQL connected!");
        let claim_store: Arc<PostgresStore> = Arc::new(postgres_store);
        let memory_layer_store: Arc<dyn cogkos_store::MemoryLayerStore> = claim_store.clone();
        Stores::new(
            claim_store,
            Arc::new(cogkos_store::vector::InMemoryVectorStore::new()),
            Arc::new(InMemoryGraphStore::new()),
            Arc::new(InMemoryCacheStore::new()),
            Arc::new(cogkos_store::InMemoryFeedbackStore::new()),
            object_store,
            Arc::new(auth_store),
            Arc::new(InMemoryGapStore::new()),
            Arc::new(cogkos_core::audit::InMemoryAuditStore::with_default_capacity()),
            Arc::new(InMemorySubscriptionStore::new()),
            memory_layer_store,
        )
    } else {
        println!("⚠️ DATABASE_URL empty, using InMemory storage");
        Stores::new(
            Arc::new(cogkos_store::InMemoryClaimStore::new()),
            Arc::new(cogkos_store::vector::InMemoryVectorStore::new()),
            Arc::new(InMemoryGraphStore::new()),
            Arc::new(InMemoryCacheStore::new()),
            Arc::new(cogkos_store::InMemoryFeedbackStore::new()),
            object_store,
            Arc::new(auth_store),
            Arc::new(InMemoryGapStore::new()),
            Arc::new(cogkos_core::audit::InMemoryAuditStore::with_default_capacity()),
            Arc::new(InMemorySubscriptionStore::new()),
            Arc::new(cogkos_store::NoopMemoryLayerStore),
        )
    };

    let config = McpConfig { host: "127.0.0.1".to_string(), port: 3002, ..Default::default() };

    // Create embedding client from config (falls back to env vars)
    // Supports local TEI servers (no API key needed for localhost)
    let embedding_client: Option<Arc<dyn cogkos_llm::LlmClient>> = if llm_config.is_configured("embedding") {
        let config = llm_config.get("embedding").unwrap();
        let base_url = config.base_url.as_deref().unwrap_or("http://localhost:8090/v1"); // verified: 2026-03-22
        // For local TEI, use empty string as API key (no auth needed)
        let api_key = if config.api_key.is_empty() && config.is_local() {
            "local".to_string() // placeholder, TEI ignores Authorization header
        } else {
            config.api_key.clone()
        };
        let mut embed_builder = cogkos_llm::LlmClientBuilder::new(&api_key, cogkos_llm::ProviderType::OpenAi)
            .with_base_url(base_url)
            .with_model(&config.model);
        match embed_builder.build() {
            Ok(client) => {
                let mode = if config.is_local() { "local" } else { &config.provider };
                println!("  ✓ Embedding: {} ({})", config.model, mode);
                Some(client)
            }
            Err(e) => {
                println!("  ⚠ Embedding build failed: {}", e);
                None
            }
        }
    } else {
        // Fallback: try MINIMAX_API_KEY with embo-01
        let minimax_key = std::env::var("MINIMAX_API_KEY").unwrap_or_default();
        if !minimax_key.is_empty() {
            let embed_base = env::var("EMBEDDING_BASE_URL")
                .unwrap_or_else(|_| "https://api.minimax.chat/v1".to_string()); // verified: 2026-03-21
            let embed_model = env::var("EMBEDDING_MODEL")
                .unwrap_or_else(|_| "embo-01".to_string()); // verified: 2026-03-21
            let mut embed_builder = cogkos_llm::LlmClientBuilder::new(&minimax_key, cogkos_llm::ProviderType::OpenAi)
                .with_base_url(&embed_base)
                .with_model(&embed_model);
            match embed_builder.build() {
                Ok(client) => {
                    println!("  ✓ Embedding: {} (minimax fallback)", embed_model);
                    Some(client)
                }
                Err(e) => {
                    println!("  ⚠ Embedding build failed: {}", e);
                    None
                }
            }
        } else {
            println!("  ⚠ Embedding not configured (set [llm.embedding] or MINIMAX_API_KEY)");
            None
        }
    };

    println!("🚀 Starting MCP server...\n");

    match start_mcp_server(stores, config, llm_client, embedding_client).await {
        Ok(_) => println!("\n✅ Server stopped"),
        Err(e) => println!("\n❌ Server error: {:?}", e),
    }
}

/// Load LLM config from config file
fn load_llm_config() -> LlmConfig {
    // Debug: print env vars

    // Find config file
    let config_path = find_config_file();

    if let Some(path) = config_path {
        match fs::read_to_string(&path) {
            Ok(content) => {
                match toml::from_str::<toml::Value>(&content) {
                    Ok(toml) => {
                        return parse_llm_config(&toml);
                    }
                    Err(e) => {
                        println!("⚠ Config file parse error: {}, using defaults", e);
                    }
                }
            }
            Err(e) => {
                println!("⚠ Config file read error: {}, using defaults", e);
            }
        }
    } else {
        println!("⚠ Config file not found, using defaults");
    }

    LlmConfig::default()
}

/// Find config file path
fn find_config_file() -> Option<PathBuf> {
    let mut search_paths: Vec<PathBuf> = vec![
        // Current directory
        PathBuf::from("config/default.toml"),
        PathBuf::from("../config/default.toml"),
        // Project root (from bin directory)
        PathBuf::from("cogkos/config/default.toml"),
    ];

    // Add cwd and parent directory configs
    if let Ok(cwd) = env::current_dir() {
        search_paths.push(cwd.join("config/default.toml"));
        // Try parent directory
        if let Some(parent) = cwd.parent() {
            search_paths.push(parent.join("config/default.toml"));
            // One more level up
            if let Some(grandparent) = parent.parent() {
                search_paths.push(grandparent.join("config/default.toml"));
            }
        }
    }

    for path in search_paths {
        if path.exists() {
            println!("  📄 Found config file: {:?}", path);
            return Some(path);
        }
    }

    None
}

/// Parse LLM config from toml value
fn parse_llm_config(toml: &toml::Value) -> LlmConfig {
    // [llm.text] parses as toml.get("llm").and_then(|t| t.get("text"))
    let llm_table = toml.get("llm");

    let text_cfg = llm_table.and_then(|t| t.get("text"));
    let embed_cfg = llm_table.and_then(|t| t.get("embedding"));
    let image_cfg = llm_table.and_then(|t| t.get("image"));
    let audio_cfg = llm_table.and_then(|t| t.get("audio"));
    let other_cfg = llm_table.and_then(|t| t.get("other"));

    // Parse configs
    let text = text_cfg.and_then(|v| parse_provider_config(Some(v)));
    let embedding = embed_cfg.and_then(|v| parse_provider_config(Some(v)));
    let image = image_cfg.and_then(|v| parse_provider_config(Some(v)));
    let audio = audio_cfg.and_then(|v| parse_provider_config(Some(v)));
    let other = other_cfg.and_then(|v| parse_provider_config(Some(v)));

    // Debug: print embedding config
    if let Some(ref e) = embedding {
    } else {
    }

    // Extract provider names from parsed results
    LlmConfig::from_toml_config(
        text.as_ref().map(|(p, m, k, b)| (p.clone(), m.clone(), k.clone(), b.clone())),
        embedding.as_ref().map(|(p, m, k, b)| (p.clone(), m.clone(), k.clone(), b.clone())),
        image.as_ref().map(|(p, m, k, b)| (p.clone(), m.clone(), k.clone(), b.clone())),
        audio.as_ref().map(|(p, m, k, b)| (p.clone(), m.clone(), k.clone(), b.clone())),
        other.as_ref().map(|(p, m, k, b)| (p.clone(), m.clone(), k.clone(), b.clone())),
    )
}

/// Get API key from env var
fn get_api_key_from_env(provider: &str) -> Option<String> {
    let env_var = match provider {
        "kimi" => "KIMI_API_KEY",
        "minimax" => "MINIMAX_API_KEY",
        "302ai" => "AI302_API_KEY",
        "doubao" => "DOUBAO_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        _ => return None,
    };
    std::env::var(env_var).ok()
}

/// Parse single provider config (env var takes priority)
fn parse_provider_config(value: Option<&toml::Value>) -> Option<(String, String, String, Option<String>)> {
    let table = value?.as_table()?;

    let provider = table.get("provider")?.as_str()?.to_string();
    let model = table.get("model")?.as_str()?.to_string();
    let base_url = table.get("base_url").and_then(|v| v.as_str()).map(|s| s.to_string());

    // Prefer API key from env var
    let api_key = get_api_key_from_env(&provider)
        .or_else(|| table.get("api_key").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .unwrap_or_default();

    Some((provider, model, api_key, base_url))
}
