#!/bin/bash
# CogKOS one-click deploy script
# Usage: ./scripts/deploy.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

echo "=== CogKOS Production Deploy ==="

# 1. Check Docker
if ! command -v docker &>/dev/null; then
    echo "❌ Docker not installed"; exit 1
fi
if ! docker compose version &>/dev/null; then
    echo "❌ Docker Compose not installed"; exit 1
fi
echo "✅ Docker $(docker --version | cut -d' ' -f3)"

# 2. Check .env
if [ ! -f .env ]; then
    echo "⚠️  .env not found, creating from template..."
    cp .env.example .env
    echo "  Please edit .env and fill in API_302_KEY etc."
fi

# 3. Start infrastructure
echo ""
echo "[1/4] Starting PostgreSQL + FalkorDB..."
docker compose up -d postgres falkordb
echo "  Waiting for health checks..."
sleep 5
docker compose exec postgres pg_isready -U cogkos -d cogkos >/dev/null && echo "  ✅ PostgreSQL ready"
docker compose exec falkordb redis-cli PING >/dev/null && echo "  ✅ FalkorDB ready"

# 4. Build and start CogKOS
echo ""
echo "[2/4] Building CogKOS container..."
docker compose build cogkos

echo ""
echo "[3/4] Starting CogKOS..."
docker compose up -d cogkos
echo "  Waiting for startup..."
sleep 10

# 5. Health check
echo ""
echo "[4/4] Health check..."
if curl -sf http://localhost:8081/healthz >/dev/null 2>&1; then
    echo "  ✅ Health: OK"
else
    echo "  ❌ Health check failed"
    docker compose logs cogkos --tail 20
    exit 1
fi

if curl -sf http://localhost:8081/readyz >/dev/null 2>&1; then
    echo "  ✅ Ready: OK"
else
    echo "  ⚠️  Readiness check failed (DB may still be migrating)"
fi

# 6. Create initial API Key (if DB is empty)
KEY_COUNT=$(docker compose exec -T postgres psql -U cogkos -d cogkos -tAc "SELECT count(*) FROM api_keys" 2>/dev/null || echo "0")
if [ "$KEY_COUNT" = "0" ]; then
    echo ""
    echo "  Creating initial API Key..."
    docker compose exec cogkos /app/cogkos-admin create-key default read,write,admin 2>/dev/null || true
fi

echo ""
echo "=== Deploy complete ==="
echo "  MCP:     http://localhost:3000/mcp"
echo "  Health:  http://localhost:8081/healthz"
echo "  Metrics: http://localhost:8081/metrics"
echo ""
echo "  Management commands:"
echo "    docker compose logs -f cogkos    # View logs"
echo "    docker compose restart cogkos    # Restart service"
echo "    docker compose down              # Stop all"
