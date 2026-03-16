use std::env;

fn main() {
    println!("CogKOS v0.1.0 - 知识操作系统");
    println!("============================");
    println!();

    // 显示版本信息
    println!("可用模块:");
    println!("  - cogkos-core: 核心功能");
    println!("  - cogkos-ingest: 文件摄取");
    println!("  - cogkos-llm: LLM 集成");
    println!("  - cogkos-workflow: 工作流引擎");
    println!("  - cogkos-store: 存储层");
    println!();

    // 检查环境变量
    println!("环境配置:");
    if env::var("MINIMAX_API_KEY").is_ok() {
        println!("  ✓ MINIMAX_API_KEY 已配置");
    } else {
        println!("  ✗ MINIMAX_API_KEY 未配置");
    }

    if env::var("DATABASE_URL").is_ok() {
        println!("  ✓ DATABASE_URL 已配置");
    } else {
        println!("  ✗ DATABASE_URL 未配置");
    }

    println!();
    println!("更多信息请访问: https://github.com/Kingxiao/cogkos");
}
