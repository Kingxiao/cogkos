#!/usr/bin/env bash
# CogKOS data backup
# Usage: bash scripts/backup.sh [backup_dir]
# Creates: cogkos_backup_YYYYMMDD_HHMMSS.tar.gz

set -euo pipefail

# ── Defaults ──
BACKUP_DIR="${1:-./backups}"
PG_CONTAINER="${PG_CONTAINER:-cogkos-postgres}"
PG_USER="${PG_USER:-cogkos}"
PG_DB="${PG_DB:-cogkos}"
FALKORDB_CONTAINER="${FALKORDB_CONTAINER:-cogkos-falkordb}"
FALKORDB_DATA="${FALKORDB_DATA:-data/falkordb}"
LOCAL_STORAGE="${LOCAL_STORAGE:-local-storage}"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
WORK_DIR=$(mktemp -d)
ARCHIVE_NAME="cogkos_backup_${TIMESTAMP}.tar.gz"

cleanup() {
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

# ── Help ──
if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
    cat <<EOF
CogKOS data backup

Usage: bash scripts/backup.sh [backup_dir]

Creates a compressed archive containing:
  - PostgreSQL dump (via docker exec ${PG_CONTAINER})
  - FalkorDB persistence files (${FALKORDB_DATA}/)
  - Local storage files (${LOCAL_STORAGE}/)

Output: <backup_dir>/cogkos_backup_YYYYMMDD_HHMMSS.tar.gz

Environment variables:
  PG_CONTAINER       Docker container name for PostgreSQL (default: cogkos-postgres)
  PG_USER            PostgreSQL user (default: cogkos)
  PG_DB              PostgreSQL database (default: cogkos)
  FALKORDB_CONTAINER Docker container name for FalkorDB (default: cogkos-falkordb)
  FALKORDB_DATA      FalkorDB data directory (default: data/falkordb)
  LOCAL_STORAGE      Local storage directory (default: local-storage)
EOF
    exit 0
fi

mkdir -p "$BACKUP_DIR"

echo "[$(date)] Starting CogKOS backup..."

# ── 1. PostgreSQL dump ──
echo "  Dumping PostgreSQL..."
PG_DUMP_FILE="${WORK_DIR}/postgres.sql.gz"
if docker exec "$PG_CONTAINER" pg_dump -U "$PG_USER" -d "$PG_DB" --no-owner --no-privileges \
    | gzip > "$PG_DUMP_FILE"; then
    echo "  PostgreSQL dump: $(du -h "$PG_DUMP_FILE" | cut -f1)"
else
    echo "  ERROR: PostgreSQL dump failed" >&2
    exit 1
fi

# ── 2. FalkorDB persistence ──
echo "  Backing up FalkorDB..."
if [ -d "$FALKORDB_DATA" ]; then
    # Trigger BGSAVE before copying
    docker exec "$FALKORDB_CONTAINER" redis-cli BGSAVE > /dev/null 2>&1 || true
    sleep 2
    mkdir -p "${WORK_DIR}/falkordb"
    cp -r "$FALKORDB_DATA"/* "${WORK_DIR}/falkordb/" 2>/dev/null || true
    echo "  FalkorDB data copied"
else
    echo "  WARN: FalkorDB data directory not found at ${FALKORDB_DATA}, skipping"
fi

# ── 3. Local storage ──
echo "  Backing up local storage..."
if [ -d "$LOCAL_STORAGE" ]; then
    mkdir -p "${WORK_DIR}/local-storage"
    cp -r "$LOCAL_STORAGE"/* "${WORK_DIR}/local-storage/" 2>/dev/null || true
    echo "  Local storage copied"
else
    echo "  WARN: Local storage directory not found at ${LOCAL_STORAGE}, skipping"
fi

# ── 4. Archive ──
echo "  Creating archive..."
ARCHIVE_PATH="${BACKUP_DIR}/${ARCHIVE_NAME}"
tar -czf "$ARCHIVE_PATH" -C "$WORK_DIR" .

# ── 5. Summary ──
ARCHIVE_SIZE=$(du -h "$ARCHIVE_PATH" | cut -f1)
echo ""
echo "[$(date)] Backup complete"
echo "  File: $(realpath "$ARCHIVE_PATH")"
echo "  Size: ${ARCHIVE_SIZE}"
