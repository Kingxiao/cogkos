use std::env;

fn main() {
    println!("CogKOS v0.1.0 - Cognitive Knowledge OS");
    println!("============================");
    println!();

    // Show available modules
    println!("Available modules:");
    println!("  - cogkos-core: Core functionality");
    println!("  - cogkos-ingest: File ingestion");
    println!("  - cogkos-llm: LLM integration");
    println!("  - cogkos-workflow: Workflow engine");
    println!("  - cogkos-store: Storage layer");
    println!();

    // Check environment variables
    println!("Environment config:");
    if env::var("MINIMAX_API_KEY").is_ok() {
        println!("  ✓ MINIMAX_API_KEY configured");
    } else {
        println!("  ✗ MINIMAX_API_KEY not configured");
    }

    if env::var("DATABASE_URL").is_ok() {
        println!("  ✓ DATABASE_URL configured");
    } else {
        println!("  ✗ DATABASE_URL not configured");
    }

    println!();
    println!("More info: https://github.com/Kingxiao/cogkos");
}
