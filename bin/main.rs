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
    println!("CogKOS v0.1.0 - 知识操作系统");
    println!("============================");
    println!();
    
    println!("可用模块:");
    println!("  - cogkos-core: 核心功能");
    println!("  - cogkos-ingest: 文件摄取");
    println!("  - cogkos-llm: LLM 集成");
    println!("  - cogkos-workflow: 工作流引擎");
    println!("  - cogkos-store: 存储层");
    println!();
    
    // 加载 LLM 配置
    let llm_config = load_llm_config();
    println!("LLM 配置: {}", llm_config);
    
    // 创建 LLM 客户端
    let llm_client: Option<Arc<dyn cogkos_llm::LlmClient>> = if llm_config.is_configured("text") {
        let config = llm_config.get("text").unwrap();
        let mut builder = LlmClientBuilder::new(&config.api_key, ProviderType::OpenAi)
            .with_base_url(config.base_url.as_deref().unwrap_or("https://api.moonshot.cn/v1"))
            .with_model(&config.model);
        match builder.build()
        {
            Ok(client) => {
                println!("  ✓ Text LLM: {} ({})", config.model, config.provider);
                Some(client)
            }
            Err(e) => {
                println!("  ⚠ Text LLM 构建失败: {}", e);
                None
            }
        }
    } else {
        println!("  ⚠ Text LLM 未配置 (需要 KIMI_API_KEY)");
        None
    };
    
    // 检查其他 LLM 配置
    if llm_config.is_configured("embedding") {
        let config = llm_config.get("embedding").unwrap();
        println!("  ✓ Embedding: {} ({})", config.model, config.provider);
    } else {
        println!("  ⚠ Embedding 未配置 (需要 AI302_API_KEY)");
    }
    
    if llm_config.is_configured("image") {
        let config = llm_config.get("image").unwrap();
        println!("  ✓ Image: {} ({})", config.model, config.provider);
    } else {
        println!("  ⚠ Image 未配置 (需要 DOUBAO_API_KEY)");
    }
    
    if llm_config.is_configured("audio") {
        let config = llm_config.get("audio").unwrap();
        println!("  ✓ Audio: {} ({})", config.model, config.provider);
    } else {
        println!("  ⚠ Audio 未配置 (需要 OPENAI_API_KEY)");
    }
    
    if llm_config.is_configured("other") {
        let config = llm_config.get("other").unwrap();
        println!("  ✓ Other: {} ({})", config.model, config.provider);
    } else {
        println!("  ⚠ Other 未配置 (需要 OPENROUTER_API_KEY)");
    }
    
    println!();
    println!("环境配置:");
    println!("  DATABASE_URL: {}", env::var("DATABASE_URL").unwrap_or_else(|_| "✗ 未配置".to_string()));
    println!("  QDRANT_URL: {}", env::var("QDRANT_URL").unwrap_or_else(|_| "✗ 未配置".to_string()));
    println!("  FALKORDB_URL: {}", env::var("FALKORDB_URL").unwrap_or_else(|_| "✗ 未配置".to_string()));
    println!();
    
    // 创建 Stores - 使用固定测试 key
    let auth_store = InMemoryAuthStoreWithKey::new();
    let test_key = auth_store.create_api_key("test-tenant", vec!["read".to_string(), "write".to_string()]).await.unwrap();
    println!("  ✓ 测试 API key: {}\n", test_key);
    
    let object_store: Arc<dyn cogkos_store::ObjectStore> = match LocalStore::new("cogkos").await {
        Ok(store) => Arc::new(store),
        Err(e) => {
            println!("❌ 对象存储初始化失败: {:?}", e);
            return;
        }
    };
    
    // Use PostgreSQL for persistence if DATABASE_URL is set
    let db_url = std::env::var("DATABASE_URL").unwrap_or_default();
    let stores = if !db_url.is_empty() {
        println!("🔄 正在连接 PostgreSQL...");
        let postgres_store = cogkos_store::PostgresStore::from_url(&db_url)
            .await
            .expect("Failed to connect to PostgreSQL");
        println!("✅ PostgreSQL 连接成功!");
        Stores::new(
            Arc::new(postgres_store),
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
    } else {
        println!("⚠️ DATABASE_URL 为空，使用 InMemory 存储");
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
    
    // 调试：打印 embedding 配置
    if let Some(embed_cfg) = llm_config.get("embedding") {
    }
    
    // 创建 embedding client
    // 强制从环境变量创建 embedding client
    let minimax_key = std::env::var("MINIMAX_API_KEY").unwrap_or_default();
    let kimi_key = std::env::var("KIMI_API_KEY").unwrap_or_default();
    let key = if !minimax_key.is_empty() { minimax_key } else { kimi_key };
    
    let embedding_client: Option<Arc<dyn cogkos_llm::LlmClient>> = if !key.is_empty() {
        let mut embed_builder = cogkos_llm::LlmClientBuilder::new(&key, cogkos_llm::ProviderType::OpenAi)
            .with_base_url("https://api.minimax.chat/v1")
            .with_model("embo-01");
        match embed_builder.build()
        {
            Ok(client) => {
                println!("  ✓ Embedding: embo-01 (minimax)");
                Some(client)
            }
            Err(e) => {
                println!("  ⚠ Embedding 构建失败: {}", e);
                None
            }
        }
    } else {
        println!("  ⚠ Embedding 未配置 (需要 MINIMAX_API_KEY)");
        None
    };
    
    println!("🚀 启动 MCP 服务器...\n");
    
    match start_mcp_server(stores, config, llm_client, embedding_client).await {
        Ok(_) => println!("\n✅ 服务器已停止"),
        Err(e) => println!("\n❌ 服务器错误: {:?}", e),
    }
}

/// 从配置文件加载 LLM 配置
fn load_llm_config() -> LlmConfig {
    // 调试：打印环境变量
    
    // 查找配置文件
    let config_path = find_config_file();
    
    if let Some(path) = config_path {
        match fs::read_to_string(&path) {
            Ok(content) => {
                match toml::from_str::<toml::Value>(&content) {
                    Ok(toml) => {
                        return parse_llm_config(&toml);
                    }
                    Err(e) => {
                        println!("⚠ 配置文件解析失败: {}, 使用默认值", e);
                    }
                }
            }
            Err(e) => {
                println!("⚠ 配置文件读取失败: {}, 使用默认值", e);
            }
        }
    } else {
        println!("⚠ 未找到配置文件, 使用默认值");
    }
    
    LlmConfig::default()
}

/// 查找配置文件路径
fn find_config_file() -> Option<PathBuf> {
    let mut search_paths: Vec<PathBuf> = vec![
        // 当前目录
        PathBuf::from("config/default.toml"),
        PathBuf::from("../config/default.toml"),
        // 项目根目录 (从 bin 目录)
        PathBuf::from("cogkos/config/default.toml"),
    ];
    
    // 添加当前工作目录及父目录的配置
    if let Ok(cwd) = env::current_dir() {
        search_paths.push(cwd.join("config/default.toml"));
        // 尝试父目录
        if let Some(parent) = cwd.parent() {
            search_paths.push(parent.join("config/default.toml"));
            // 再上一级
            if let Some(grandparent) = parent.parent() {
                search_paths.push(grandparent.join("config/default.toml"));
            }
        }
    }
    
    for path in search_paths {
        if path.exists() {
            println!("  📄 找到配置文件: {:?}", path);
            return Some(path);
        }
    }
    
    None
}

/// 从 toml 值解析 LLM 配置
fn parse_llm_config(toml: &toml::Value) -> LlmConfig {
    // [llm.text] 会解析为 toml.get("llm").and_then(|t| t.get("text"))
    let llm_table = toml.get("llm");
    
    let text_cfg = llm_table.and_then(|t| t.get("text"));
    let embed_cfg = llm_table.and_then(|t| t.get("embedding"));
    let image_cfg = llm_table.and_then(|t| t.get("image"));
    let audio_cfg = llm_table.and_then(|t| t.get("audio"));
    let other_cfg = llm_table.and_then(|t| t.get("other"));
    
    // 解析配置
    let text = text_cfg.and_then(|v| parse_provider_config(Some(v)));
    let embedding = embed_cfg.and_then(|v| parse_provider_config(Some(v)));
    let image = image_cfg.and_then(|v| parse_provider_config(Some(v)));
    let audio = audio_cfg.and_then(|v| parse_provider_config(Some(v)));
    let other = other_cfg.and_then(|v| parse_provider_config(Some(v)));
    
    // 调试：打印 embedding 配置
    if let Some(ref e) = embedding {
    } else {
    }
    
    // 从解析结果中提取 provider 名称
    LlmConfig::from_toml_config(
        text.as_ref().map(|(p, m, k, b)| (p.clone(), m.clone(), k.clone(), b.clone())),
        embedding.as_ref().map(|(p, m, k, b)| (p.clone(), m.clone(), k.clone(), b.clone())),
        image.as_ref().map(|(p, m, k, b)| (p.clone(), m.clone(), k.clone(), b.clone())),
        audio.as_ref().map(|(p, m, k, b)| (p.clone(), m.clone(), k.clone(), b.clone())),
        other.as_ref().map(|(p, m, k, b)| (p.clone(), m.clone(), k.clone(), b.clone())),
    )
}

/// 从环境变量获取 API key
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

/// 解析单个 provider 配置（环境变量优先）
fn parse_provider_config(value: Option<&toml::Value>) -> Option<(String, String, String, Option<String>)> {
    let table = value?.as_table()?;
    
    let provider = table.get("provider")?.as_str()?.to_string();
    let model = table.get("model")?.as_str()?.to_string();
    let base_url = table.get("base_url").and_then(|v| v.as_str()).map(|s| s.to_string());
    
    // 优先从环境变量获取 API key
    let api_key = get_api_key_from_env(&provider)
        .or_else(|| table.get("api_key").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .unwrap_or_default();
    
    Some((provider, model, api_key, base_url))
}
