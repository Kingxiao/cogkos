#!/bin/bash
# CogKOS 一键部署脚本
# 用法: ./scripts/deploy.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

echo "=== CogKOS 生产部署 ==="

# 1. 检查 Docker
if ! command -v docker &>/dev/null; then
    echo "❌ Docker 未安装"; exit 1
fi
if ! docker compose version &>/dev/null; then
    echo "❌ Docker Compose 未安装"; exit 1
fi
echo "✅ Docker $(docker --version | cut -d' ' -f3)"

# 2. 检查 .env
if [ ! -f .env ]; then
    echo "⚠️  .env 不存在，从模板创建..."
    cp .env.example .env
    echo "  请编辑 .env 填入 API_302_KEY 等配置"
fi

# 3. 启动基础设施
echo ""
echo "[1/4] 启动 PostgreSQL + FalkorDB..."
docker compose up -d postgres falkordb
echo "  等待健康检查..."
sleep 5
docker compose exec postgres pg_isready -U cogkos -d cogkos >/dev/null && echo "  ✅ PostgreSQL ready"
docker compose exec falkordb redis-cli PING >/dev/null && echo "  ✅ FalkorDB ready"

# 4. 构建并启动 CogKOS
echo ""
echo "[2/4] 构建 CogKOS 容器..."
docker compose build cogkos

echo ""
echo "[3/4] 启动 CogKOS..."
docker compose up -d cogkos
echo "  等待启动..."
sleep 10

# 5. 健康检查
echo ""
echo "[4/4] 健康检查..."
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

# 6. 创建初始 API Key（如果 DB 为空）
KEY_COUNT=$(docker compose exec -T postgres psql -U cogkos -d cogkos -tAc "SELECT count(*) FROM api_keys" 2>/dev/null || echo "0")
if [ "$KEY_COUNT" = "0" ]; then
    echo ""
    echo "  创建初始 API Key..."
    docker compose exec cogkos /app/cogkos-admin create-key default read,write,admin 2>/dev/null || true
fi

echo ""
echo "=== 部署完成 ==="
echo "  MCP:     http://localhost:3000/mcp"
echo "  Health:  http://localhost:8081/healthz"
echo "  Metrics: http://localhost:8081/metrics"
echo ""
echo "  管理命令:"
echo "    docker compose logs -f cogkos    # 查看日志"
echo "    docker compose restart cogkos    # 重启服务"
echo "    docker compose down              # 停止全部"
