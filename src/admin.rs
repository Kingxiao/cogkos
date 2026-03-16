//! CogKOS Admin CLI - API key management

use anyhow::Result;
use cogkos_store::AuthStore;
use cogkos_store::postgres::PostgresStore;
use sqlx::postgres::PgPoolOptions;
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "create-key" => {
            let tenant_id = args.get(2).map(|s| s.as_str()).unwrap_or("default");
            let permissions: Vec<String> = if args.len() > 3 {
                args[3].split(',').map(|s| s.trim().to_string()).collect()
            } else {
                vec!["read".to_string(), "write".to_string()]
            };

            let store = connect_db().await?;
            let api_key = store.create_api_key(tenant_id, permissions.clone()).await?;

            println!("API key created successfully:");
            println!("  Key:         {}", api_key);
            println!("  Tenant:      {}", tenant_id);
            println!("  Permissions: {:?}", permissions);
            println!();
            println!("Store this key securely - it cannot be retrieved later.");
        }
        "revoke-key" => {
            let key_hash = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: cogkos-admin revoke-key <key_hash>"))?;

            let store = connect_db().await?;
            store.revoke_api_key(key_hash).await?;
            println!("API key revoked: {}", key_hash);
        }
        "list-keys" => {
            let pool = connect_pool().await?;
            let rows = sqlx::query_as::<_, (String, String, Vec<String>, bool, chrono::DateTime<chrono::Utc>)>(
                "SELECT key_hash, tenant_id, permissions, enabled, created_at FROM api_keys ORDER BY created_at DESC",
            )
            .fetch_all(&pool)
            .await?;

            if rows.is_empty() {
                println!("No API keys found.");
            } else {
                println!(
                    "{:<36} {:<16} {:<24} {:<8} CREATED",
                    "HASH", "TENANT", "PERMISSIONS", "ENABLED"
                );
                println!("{}", "-".repeat(100));
                for (hash, tenant, perms, enabled, created) in &rows {
                    println!(
                        "{:<36} {:<16} {:<24} {:<8} {}",
                        hash,
                        tenant,
                        format!("{:?}", perms),
                        enabled,
                        created.format("%Y-%m-%d %H:%M")
                    );
                }
            }
        }
        "check-db" => {
            let pool = connect_pool().await?;
            let row: (i64,) = sqlx::query_as("SELECT 1").fetch_one(&pool).await?;
            println!("Database connection OK (result: {})", row.0);
        }
        _ => {
            print_usage();
            std::process::exit(1);
        }
    }

    Ok(())
}

fn print_usage() {
    eprintln!("CogKOS Admin CLI");
    eprintln!();
    eprintln!("Usage: cogkos-admin <command> [args]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  create-key [tenant_id] [permissions]  Create API key (default: read,write)");
    eprintln!("  revoke-key <key_hash>                 Revoke an API key");
    eprintln!("  list-keys                             List all API keys");
    eprintln!("  check-db                              Verify database connectivity");
    eprintln!();
    eprintln!("Environment:");
    eprintln!("  DATABASE_URL  PostgreSQL connection URL (required)");
}

async fn connect_pool() -> Result<sqlx::PgPool> {
    let url = env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL environment variable not set"))?;

    PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&url)
        .await
        .map_err(|e| anyhow::anyhow!("Database connection failed: {}", e))
}

async fn connect_db() -> Result<PostgresStore> {
    let pool = connect_pool().await?;
    Ok(PostgresStore::new(pool))
}
