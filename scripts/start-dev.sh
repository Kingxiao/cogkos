#!/bin/bash
# CogKOS dogfooding dev environment startup script

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo "=== CogKOS Dogfooding Startup ==="

# 1. Check dependency containers
echo "[1/4] Checking infrastructure..."
if ! docker exec cogkos-postgres pg_isready -U cogkos -d cogkos > /dev/null 2>&1; then
    echo "  ⚠️  PostgreSQL not running, please run:"
    echo "  docker run -d --name cogkos-postgres -p 5435:5432 -e POSTGRES_USER=cogkos -e POSTGRES_PASSWORD=cogkos_dev -e POSTGRES_DB=cogkos -v $PROJECT_DIR/data/postgres:/var/lib/postgresql/data --restart unless-stopped pgvector/pgvector:pg17"
    exit 1
fi
echo "  ✅ PostgreSQL on :5435"

if ! docker exec cogkos-falkordb redis-cli PING > /dev/null 2>&1; then
    echo "  ⚠️  FalkorDB not running, please run:"
    echo "  docker run -d --name cogkos-falkordb -p 6381:6379 -v $PROJECT_DIR/data/falkordb:/data --restart unless-stopped falkordb/falkordb:latest"
    exit 1
fi
echo "  ✅ FalkorDB on :6381"

# 2. Environment variables
echo "[2/4] Loading environment variables..."
export DATABASE_URL="postgres://cogkos:cogkos_dev@localhost:5435/cogkos"
export FALKORDB_URL="redis://localhost:6381"
export FALKORDB_GRAPH="cogkos"
export MCP_TRANSPORT="http"
export MCP_PORT="3000"
export HEALTH_PORT="8081"
export RUST_LOG="info"
export EMBEDDING_BASE_URL="${EMBEDDING_BASE_URL:-http://localhost:8090/v1}"
export EMBEDDING_MODEL="${EMBEDDING_MODEL:-BAAI/bge-m3}"

# Check embedding availability
if [ -n "$EMBEDDING_API_KEY" ] || [ -n "$API_302_KEY" ]; then
    echo "  ✅ Embedding API key configured"
elif curl -sf http://localhost:8090/health >/dev/null 2>&1; then
    echo "  ✅ Local TEI (BGE-M3) running"
else
    echo "  ⚠️  No embedding available (start TEI: docker compose -f docker-compose.bge-m3.yml up -d)"
fi

# 3. Build (if needed)
BINARY="$PROJECT_DIR/target/release/cogkos"
if [ ! -f "$BINARY" ] || [ "$PROJECT_DIR/src/main.rs" -nt "$BINARY" ]; then
    echo "[3/4] Building release..."
    cd "$PROJECT_DIR"
    cargo build --release
else
    echo "[3/4] Binary is up to date"
fi

# 4. Start
echo "[4/4] Starting CogKOS..."
echo "  MCP: http://localhost:3000/mcp"
echo "  Health: http://localhost:8081/healthz"
echo "  Press Ctrl+C to stop"
echo ""

exec "$BINARY"
