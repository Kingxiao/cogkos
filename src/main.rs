//! CogKOS - Cognitive Knowledge Operating System

use anyhow::Result;
use cogkos_llm::{LlmClientBuilder, ProviderType};
use cogkos_mcp::{McpConfig, start_mcp_server};
use cogkos_store::{Stores, postgres::PostgresStore, postgres_audit::PostgresAuditStore};
use config::{Config, File, FileFormat};
use serde::Deserialize;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt};

/// Application configuration
#[derive(Clone, Debug, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub falkordb: FalkorDbConfig,
    #[serde(default)]
    pub s3: S3Config,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    #[serde(default = "default_request_timeout")]
    pub request_timeout_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            max_connections: default_max_connections(),
            request_timeout_secs: default_request_timeout(),
        }
    }
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}
fn default_port() -> u16 {
    3000
}
fn default_max_connections() -> usize {
    10000
}
fn default_request_timeout() -> u64 {
    30
}

#[derive(Clone, Debug, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_ttl")]
    pub ttl_seconds: i64,
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            ttl_seconds: default_ttl(),
            max_entries: default_max_entries(),
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_ttl() -> i64 {
    3600
}
fn default_max_entries() -> usize {
    10000
}

#[derive(Clone, Debug, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_database_url")]
    pub url: String,
    #[serde(default = "default_pg_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_pg_min_connections")]
    pub min_connections: u32,
    #[serde(default = "default_pg_idle_timeout")]
    pub idle_timeout_secs: u64,
    #[serde(default = "default_pg_acquire_timeout")]
    pub acquire_timeout_secs: u64,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: default_database_url(),
            max_connections: default_pg_max_connections(),
            min_connections: default_pg_min_connections(),
            idle_timeout_secs: default_pg_idle_timeout(),
            acquire_timeout_secs: default_pg_acquire_timeout(),
        }
    }
}

fn default_database_url() -> String {
    "postgres://localhost:5432/cogkos".to_string()
}
fn default_pg_max_connections() -> u32 {
    20
}
fn default_pg_min_connections() -> u32 {
    2
}
fn default_pg_idle_timeout() -> u64 {
    600
}
fn default_pg_acquire_timeout() -> u64 {
    5
}

#[derive(Clone, Debug, Deserialize)]
pub struct FalkorDbConfig {
    #[serde(default = "default_falkordb_url")]
    pub url: String,
    #[serde(default = "default_falkordb_graph")]
    pub graph_name: String,
}

impl Default for FalkorDbConfig {
    fn default() -> Self {
        Self {
            url: default_falkordb_url(),
            graph_name: default_falkordb_graph(),
        }
    }
}

fn default_falkordb_url() -> String {
    "redis://localhost:6379".to_string()
}
fn default_falkordb_graph() -> String {
    "cogkos".to_string()
}

#[derive(Clone, Debug, Deserialize)]
pub struct S3Config {
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default = "default_s3_region")]
    pub region: String,
    #[serde(default = "default_s3_bucket")]
    pub bucket: String,
    #[serde(default = "default_s3_access_key")]
    pub access_key: String,
    #[serde(default = "default_s3_secret_key")]
    pub secret_key: String,
}

impl Default for S3Config {
    fn default() -> Self {
        Self {
            endpoint: None,
            region: default_s3_region(),
            bucket: default_s3_bucket(),
            access_key: default_s3_access_key(),
            secret_key: default_s3_secret_key(),
        }
    }
}

fn default_s3_region() -> String {
    "us-east-1".to_string()
}
fn default_s3_bucket() -> String {
    "cogkos-docs".to_string()
}
fn default_s3_access_key() -> String {
    String::new()
}
fn default_s3_secret_key() -> String {
    String::new()
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct SecurityConfig {
    #[serde(default)]
    pub cors_allow_origins: Vec<String>,
    #[serde(default = "default_rate_limit")]
    pub rate_limit_requests_per_minute: usize,
}

fn default_rate_limit() -> usize {
    1000
}

#[derive(Clone, Debug, Deserialize)]
pub struct TelemetryConfig {
    #[serde(default = "default_true")]
    pub metrics_enabled: bool,
    #[serde(default = "default_true")]
    pub tracing_enabled: bool,
    #[serde(default)]
    pub jaeger_endpoint: Option<String>,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            metrics_enabled: default_true(),
            tracing_enabled: default_true(),
            jaeger_endpoint: None,
        }
    }
}

impl AppConfig {
    /// Validate configuration at startup
    fn validate(&self) -> Result<()> {
        if self.server.port == 0 {
            anyhow::bail!("server.port must be non-zero");
        }
        if self.server.max_connections == 0 {
            anyhow::bail!("server.max_connections must be non-zero");
        }
        if self.database.max_connections == 0 {
            anyhow::bail!("database.max_connections must be non-zero");
        }
        if self.database.max_connections < self.database.min_connections {
            anyhow::bail!("database.max_connections must be >= min_connections");
        }
        if self.database.url.is_empty() {
            anyhow::bail!("database.url must be set");
        }
        if self.falkordb.url.is_empty() {
            anyhow::bail!("falkordb.url must be set");
        }
        Ok(())
    }
}

/// Load configuration from file and environment
fn load_config() -> Result<AppConfig> {
    let config_dir = std::path::Path::new("config");

    let mut builder = Config::builder();

    // Load default config file if exists
    let config_path = config_dir.join("default.toml");
    if config_path.exists() {
        builder = builder.add_source(File::from(config_path).format(FileFormat::Toml));
    }

    let config: AppConfig = builder.build()?.try_deserialize()?;
    config.validate()?;

    Ok(config)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging + optional OpenTelemetry tracing
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let json_logging = std::env::var("LOG_FORMAT").as_deref() == Ok("json");

    // Check if OTLP endpoint is configured (via env or config)
    let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();

    // Build optional OpenTelemetry tracer
    // Keep provider alive via variable binding (dropped at end of main → flushes spans)
    let mut _otel_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider> = None;
    let otel_tracer = if let Some(endpoint) = otlp_endpoint {
        use opentelemetry::trace::TracerProvider as _;
        use opentelemetry_otlp::WithExportConfig as _;
        use opentelemetry_sdk::trace::SdkTracerProvider;

        match opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint.clone())
            .build()
        {
            Ok(exporter) => {
                let provider = SdkTracerProvider::builder()
                    .with_batch_exporter(exporter)
                    .build();
                let tracer = provider.tracer("cogkos");
                _otel_provider = Some(provider);
                eprintln!("OpenTelemetry enabled, exporting to {}", endpoint);
                Some(tracer)
            }
            Err(e) => {
                eprintln!(
                    "Failed to create OTLP exporter: {}, continuing without OTel",
                    e
                );
                None
            }
        }
    } else {
        None
    };

    if json_logging {
        let subscriber = tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().json().with_target(true).with_thread_ids(true));
        if let Some(tracer) = otel_tracer {
            let subscriber = subscriber.with(tracing_opentelemetry::layer().with_tracer(tracer));
            tracing::subscriber::set_global_default(subscriber)?;
        } else {
            tracing::subscriber::set_global_default(subscriber)?;
        }
    } else {
        let subscriber = tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().with_target(false));
        if let Some(tracer) = otel_tracer {
            let subscriber = subscriber.with(tracing_opentelemetry::layer().with_tracer(tracer));
            tracing::subscriber::set_global_default(subscriber)?;
        } else {
            tracing::subscriber::set_global_default(subscriber)?;
        }
    }

    info!("Starting CogKOS...");

    // Load configuration
    let config = load_config().unwrap_or_else(|e| {
        info!("Failed to load config file, using defaults: {}", e);
        AppConfig {
            server: ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 3000,
                max_connections: 10000,
                request_timeout_secs: 30,
            },
            cache: CacheConfig {
                enabled: true,
                ttl_seconds: 3600,
                max_entries: 10000,
            },
            database: DatabaseConfig::default(),
            falkordb: FalkorDbConfig {
                url: default_falkordb_url(),
                graph_name: default_falkordb_graph(),
            },
            s3: S3Config::default(),
            security: SecurityConfig {
                cors_allow_origins: vec!["*".to_string()],
                rate_limit_requests_per_minute: 1000,
            },
            telemetry: TelemetryConfig {
                metrics_enabled: true,
                tracing_enabled: true,
                jaeger_endpoint: None,
            },
        }
    });

    // Override with environment variables if set
    let database_url = std::env::var("DATABASE_URL").unwrap_or(config.database.url);
    let falkordb_url = std::env::var("FALKORDB_URL").unwrap_or(config.falkordb.url);
    let falkordb_graph = std::env::var("FALKORDB_GRAPH").unwrap_or(config.falkordb.graph_name);
    let s3_endpoint = std::env::var("S3_ENDPOINT").ok().or(config.s3.endpoint);
    let s3_region = std::env::var("S3_REGION").unwrap_or(config.s3.region);
    let s3_bucket = std::env::var("S3_BUCKET").unwrap_or(config.s3.bucket);
    let s3_access_key = std::env::var("S3_ACCESS_KEY").unwrap_or(config.s3.access_key);
    let s3_secret_key = std::env::var("S3_SECRET_KEY").unwrap_or(config.s3.secret_key);

    // Connect to PostgreSQL with retry
    info!(
        max_connections = config.database.max_connections,
        min_connections = config.database.min_connections,
        "Connecting to PostgreSQL..."
    );
    let pg_pool = {
        let mut retries = 5;
        loop {
            match PgPoolOptions::new()
                .max_connections(config.database.max_connections)
                .min_connections(config.database.min_connections)
                .acquire_timeout(std::time::Duration::from_secs(
                    config.database.acquire_timeout_secs,
                ))
                .idle_timeout(std::time::Duration::from_secs(
                    config.database.idle_timeout_secs,
                ))
                .connect(&database_url)
                .await
            {
                Ok(pool) => break pool,
                Err(e) if retries > 0 => {
                    retries -= 1;
                    tracing::warn!(
                        retries_left = retries,
                        "PostgreSQL connection failed: {}",
                        e
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                }
                Err(e) => return Err(e.into()),
            }
        }
    };

    // Run database migrations
    sqlx::migrate!("./migrations")
        .run(&pg_pool)
        .await
        .map_err(|e| anyhow::anyhow!("Database migration failed: {}", e))?;
    info!("Database migrations applied");

    let pg_store = PostgresStore::new(pg_pool.clone());

    // Create stores (PostgresStore implements ClaimStore, CacheStore, FeedbackStore, AuthStore, GapStore, SubscriptionStore)
    let claim_store = Arc::new(pg_store);
    let cache_store: Arc<dyn cogkos_store::CacheStore> = claim_store.clone();
    let feedback_store: Arc<dyn cogkos_store::FeedbackStore> = claim_store.clone();
    let auth_store: Arc<dyn cogkos_store::AuthStore> = claim_store.clone();
    let gap_store: Arc<dyn cogkos_store::GapStore> = claim_store.clone();
    let subscription_store: Arc<dyn cogkos_store::SubscriptionStore> = claim_store.clone();

    // Create graph store (FalkorDB)
    info!("Connecting to FalkorDB...");
    let redis_cfg = deadpool_redis::Config::from_url(&falkordb_url);
    let redis_pool = redis_cfg
        .create_pool(Some(deadpool_redis::Runtime::Tokio1))
        .map_err(|e| anyhow::anyhow!("FalkorDB pool creation failed: {}", e))?;
    let redis_pool_health = redis_pool.clone();
    let graph_store = Arc::new(cogkos_store::FalkorStore::new(redis_pool, &falkordb_graph));

    // Create vector store (PgVectorStore with 512-dim for bge-small-zh-v1.5)
    info!("Connecting to PgVectorStore...");
    let vector_store = Arc::new(
        cogkos_store::PgVectorStore::new(pg_pool.clone(), 512)
            .await
            .map_err(|e| anyhow::anyhow!("PgVectorStore init failed: {}", e))?,
    );

    // Create object store (S3)
    info!("Connecting to S3...");
    let object_store = Arc::new(
        cogkos_store::S3Store::new(
            s3_endpoint.as_deref(),
            &s3_region,
            &s3_bucket,
            &s3_access_key,
            &s3_secret_key,
        )
        .await
        .map_err(|e| anyhow::anyhow!("S3 connection failed: {}", e))?,
    );

    // Create audit store (schema already created by migrations)
    let audit_store = Arc::new(PostgresAuditStore::new(pg_pool.clone()));

    // Create stores container
    let stores = Stores::new(
        claim_store,
        vector_store,
        graph_store,
        cache_store,
        feedback_store,
        object_store,
        auth_store,
        gap_store,
        audit_store,
        subscription_store,
    );

    let stores_arc = Arc::new(stores.clone());

    // Start sleep-time scheduler
    info!("Starting sleep-time scheduler...");
    let scheduler =
        cogkos_sleep::Scheduler::new(stores_arc, cogkos_sleep::SchedulerConfig::default());
    let scheduler_handle = scheduler.cancellation_token();
    tokio::spawn(async move {
        scheduler.start().await;
    });

    // Initialize LLM clients (optional, based on environment variables)
    let llm_client = if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
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
        info!("LLM client initialized (provider: {:?})", provider);
        Some(client)
    } else {
        info!("OPENAI_API_KEY not set, LLM features will use statistical fallback");
        None
    };

    let embedding_client = if let Ok(api_key) =
        std::env::var("EMBEDDING_API_KEY").or_else(|_| std::env::var("OPENAI_API_KEY"))
    {
        let provider = match std::env::var("EMBEDDING_PROVIDER").as_deref() {
            Ok("anthropic") => ProviderType::Anthropic,
            _ => ProviderType::OpenAi,
        };
        let mut builder = LlmClientBuilder::new(api_key, provider);
        if let Ok(base_url) = std::env::var("EMBEDDING_BASE_URL") {
            builder = builder.with_base_url(base_url);
        }
        if let Ok(model) = std::env::var("EMBEDDING_MODEL") {
            builder = builder.with_model(model);
        }
        Some(builder.build()?)
    } else {
        None
    };

    // Start MCP server (stdio transport)
    info!("Starting MCP server...");
    let mcp_config = McpConfig {
        host: std::env::var("MCP_HOST").unwrap_or(config.server.host),
        port: std::env::var("MCP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(config.server.port),
        max_connections: config.server.max_connections,
        cache_ttl_seconds: config.cache.ttl_seconds,
        cache_max_entries: config.cache.max_entries,
        rate_limit_per_minute: Some(config.security.rate_limit_requests_per_minute as u32),
    };

    // Validate S3 credentials at startup
    if s3_access_key.is_empty() || s3_secret_key.is_empty() {
        tracing::warn!(
            "S3_ACCESS_KEY or S3_SECRET_KEY is empty — object storage operations will fail"
        );
    }

    // Start health/readiness HTTP endpoint for k8s probes
    let health_port: u16 = std::env::var("HEALTH_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let health_pool = pg_pool.clone();
    let health_redis = redis_pool_health;
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = match tokio::net::TcpListener::bind(("0.0.0.0", health_port)).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(
                    port = health_port,
                    "Failed to bind health check port: {}",
                    e
                );
                return;
            }
        };
        info!(port = health_port, "Health/metrics endpoint listening");
        loop {
            if let Ok((mut stream, _)) = listener.accept().await {
                let pool = health_pool.clone();
                let redis = health_redis.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 512];
                    let _ = stream.read(&mut buf).await;
                    let req = String::from_utf8_lossy(&buf);

                    if req.contains("/readyz") {
                        // Readiness: check DB + FalkorDB connectivity
                        let pg_ok = sqlx::query("SELECT 1").fetch_one(&pool).await.is_ok();
                        let redis_ok = match redis.get().await {
                            Ok(mut conn) => {
                                use deadpool_redis::redis::AsyncCommands;
                                conn.get::<&str, Option<String>>("__health").await.is_ok()
                            }
                            Err(_) => false,
                        };
                        if pg_ok && redis_ok {
                            let _ = stream
                                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nready")
                                .await;
                        } else {
                            let body = format!("pg={} redis={}", pg_ok, redis_ok);
                            let resp = format!(
                                "HTTP/1.1 503 Service Unavailable\r\nContent-Length: {}\r\n\r\n{}",
                                body.len(),
                                body
                            );
                            let _ = stream.write_all(resp.as_bytes()).await;
                        }
                    } else if req.contains("/metrics") {
                        // Prometheus metrics: PgPool stats + application metrics
                        let mut body = format!(
                            "# HELP cogkos_pg_pool_size Current pool size\n\
                             # TYPE cogkos_pg_pool_size gauge\n\
                             cogkos_pg_pool_size {}\n\
                             # HELP cogkos_pg_pool_idle Idle connections\n\
                             # TYPE cogkos_pg_pool_idle gauge\n\
                             cogkos_pg_pool_idle {}\n\
                             # HELP cogkos_pg_pool_max Max connections\n\
                             # TYPE cogkos_pg_pool_max gauge\n\
                             cogkos_pg_pool_max {}\n",
                            pool.size(),
                            pool.num_idle(),
                            pool.options().get_max_connections(),
                        );
                        // Append application-level metrics
                        body.push_str(&cogkos_core::monitoring::METRICS.to_prometheus_text());
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(header.as_bytes()).await;
                        let _ = stream.write_all(body.as_bytes()).await;
                    } else {
                        // Liveness: always 200
                        let _ = stream
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                            .await;
                    }
                });
            }
        }
    });

    // Graceful shutdown: race MCP server against SIGTERM/SIGINT
    let shutdown_signal = async {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("failed to install SIGTERM handler");
            tokio::select! {
                _ = ctrl_c => info!("Received SIGINT, draining connections..."),
                _ = sigterm.recv() => info!("Received SIGTERM, draining connections..."),
            }
        }
        #[cfg(not(unix))]
        {
            ctrl_c.await.ok();
            info!("Received shutdown signal, draining connections...");
        }
    };

    tokio::select! {
        result = start_mcp_server(stores, mcp_config, llm_client, embedding_client) => {
            result?;
        }
        _ = shutdown_signal => {}
    }

    // Cancel all background tasks (scheduler, health check, etc.)
    info!("Cancelling background tasks...");
    scheduler_handle.cancel();
    // Give tasks a moment to wind down
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Close database pool gracefully with timeout
    info!("Closing database pool...");
    let shutdown = pg_pool.close();
    match tokio::time::timeout(std::time::Duration::from_secs(30), shutdown).await {
        Ok(()) => info!("Database pool closed gracefully"),
        Err(_) => tracing::warn!("Database pool close timed out after 30s, forcing shutdown"),
    }

    // Flush OTel spans before exit
    drop(_otel_provider);
    info!("CogKOS shutdown complete");

    Ok(())
}
