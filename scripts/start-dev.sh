#!/bin/bash
# CogKOS Dogfooding 开发环境启动脚本

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo "=== CogKOS Dogfooding 启动 ==="

# 1. 检查依赖容器
echo "[1/4] 检查基础设施..."
if ! docker exec cogkos-postgres pg_isready -U cogkos -d cogkos > /dev/null 2>&1; then
    echo "  ⚠️  PostgreSQL 未运行，请先执行:"
    echo "  docker run -d --name cogkos-postgres -p 5435:5432 -e POSTGRES_USER=cogkos -e POSTGRES_PASSWORD=cogkos_dev -e POSTGRES_DB=cogkos -v $PROJECT_DIR/data/postgres:/var/lib/postgresql/data --restart unless-stopped pgvector/pgvector:pg17"
    exit 1
fi
echo "  ✅ PostgreSQL on :5435"

if ! docker exec cogkos-falkordb redis-cli PING > /dev/null 2>&1; then
    echo "  ⚠️  FalkorDB 未运行，请先执行:"
    echo "  docker run -d --name cogkos-falkordb -p 6381:6379 -v $PROJECT_DIR/data/falkordb:/data --restart unless-stopped falkordb/falkordb:latest"
    exit 1
fi
echo "  ✅ FalkorDB on :6381"

# 2. 环境变量
echo "[2/4] 加载环境变量..."
export DATABASE_URL="postgres://cogkos:cogkos_dev@localhost:5435/cogkos"
export FALKORDB_URL="redis://localhost:6381"
export FALKORDB_GRAPH="cogkos"
export MCP_TRANSPORT="http"
export MCP_PORT="3000"
export HEALTH_PORT="8081"
export RUST_LOG="info"
export EMBEDDING_BASE_URL="https://api.302.ai/v1"
export EMBEDDING_MODEL="text-embedding-3-large"

# API key 从环境继承（~/.zshrc 中的 API_302_KEY）
if [ -z "$API_302_KEY" ]; then
    echo "  ⚠️  API_302_KEY 未设置，embedding 将使用 fallback"
else
    echo "  ✅ API_302_KEY 已配置"
fi

# 3. 编译（如需要）
BINARY="$PROJECT_DIR/target/release/cogkos"
if [ ! -f "$BINARY" ] || [ "$PROJECT_DIR/src/main.rs" -nt "$BINARY" ]; then
    echo "[3/4] 编译 release..."
    cd "$PROJECT_DIR"
    cargo build --release
else
    echo "[3/4] 二进制已是最新"
fi

# 4. 启动
echo "[4/4] 启动 CogKOS..."
echo "  MCP: http://localhost:3000/mcp"
echo "  Health: http://localhost:8081/healthz"
echo "  按 Ctrl+C 停止"
echo ""

exec "$BINARY"
