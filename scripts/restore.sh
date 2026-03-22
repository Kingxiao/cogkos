#!/usr/bin/env bash
# CogKOS data restore
# Usage: bash scripts/restore.sh <backup_file.tar.gz>

set -euo pipefail

# ── Defaults ──
PG_CONTAINER="${PG_CONTAINER:-cogkos-postgres}"
PG_USER="${PG_USER:-cogkos}"
PG_DB="${PG_DB:-cogkos}"
FALKORDB_DATA="${FALKORDB_DATA:-data/falkordb}"
LOCAL_STORAGE="${LOCAL_STORAGE:-local-storage}"
COGKOS_SERVICE="${COGKOS_SERVICE:-cogkos}"
HEALTH_URL="${HEALTH_URL:-http://localhost:8081/healthz}"
WORK_DIR=""

cleanup() {
    if [ -n "$WORK_DIR" ] && [ -d "$WORK_DIR" ]; then
        rm -rf "$WORK_DIR"
    fi
}
trap cleanup EXIT

# ── Help ──
if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
    cat <<EOF
CogKOS data restore

Usage: bash scripts/restore.sh <backup_file.tar.gz>

Restores from a backup archive created by backup.sh:
  1. Extracts the tar.gz archive
  2. Stops the CogKOS service (systemctl --user or cargo process)
  3. Restores PostgreSQL via docker exec psql
  4. Restores FalkorDB data files
  5. Restores local storage files
  6. Restarts the CogKOS service
  7. Verifies health check

Environment variables:
  PG_CONTAINER       Docker container name for PostgreSQL (default: cogkos-postgres)
  PG_USER            PostgreSQL user (default: cogkos)
  PG_DB              PostgreSQL database (default: cogkos)
  FALKORDB_DATA      FalkorDB data directory (default: data/falkordb)
  LOCAL_STORAGE      Local storage directory (default: local-storage)
  COGKOS_SERVICE     Systemd user service name (default: cogkos)
  HEALTH_URL         Health check URL (default: http://localhost:8081/healthz)
EOF
    exit 0
fi

# ── Validate input ──
BACKUP_FILE="${1:?ERROR: Usage: bash scripts/restore.sh <backup_file.tar.gz>}"

if [ ! -f "$BACKUP_FILE" ]; then
    echo "ERROR: Backup file not found: ${BACKUP_FILE}" >&2
    exit 1
fi

echo "[$(date)] Starting CogKOS restore from: ${BACKUP_FILE}"
echo "WARNING: This will overwrite current data. Press Ctrl+C within 5 seconds to abort."
sleep 5

# ── 1. Extract archive ──
echo "  Extracting archive..."
WORK_DIR=$(mktemp -d)
tar -xzf "$BACKUP_FILE" -C "$WORK_DIR"
echo "  Archive extracted to temporary directory"

# ── 2. Stop CogKOS service ──
echo "  Stopping CogKOS service..."
if systemctl --user is-active "$COGKOS_SERVICE" > /dev/null 2>&1; then
    systemctl --user stop "$COGKOS_SERVICE"
    echo "  Service stopped via systemd"
elif pkill -f "target/release/cogkos" 2>/dev/null || pkill -f "target/debug/cogkos" 2>/dev/null; then
    echo "  CogKOS process killed"
    sleep 2
else
    echo "  WARN: No running CogKOS service found, continuing"
fi

# ── 3. PostgreSQL restore ──
if [ -f "${WORK_DIR}/postgres.sql.gz" ]; then
    echo "  Restoring PostgreSQL..."

    # Terminate existing connections
    docker exec "$PG_CONTAINER" psql -U "$PG_USER" -d postgres \
        -c "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '${PG_DB}' AND pid <> pg_backend_pid();" \
        > /dev/null 2>&1 || true

    # Drop and recreate database
    docker exec "$PG_CONTAINER" psql -U "$PG_USER" -d postgres \
        -c "DROP DATABASE IF EXISTS ${PG_DB};" 2>/dev/null || true
    docker exec "$PG_CONTAINER" psql -U "$PG_USER" -d postgres \
        -c "CREATE DATABASE ${PG_DB};"

    # Restore dump
    gunzip -c "${WORK_DIR}/postgres.sql.gz" \
        | docker exec -i "$PG_CONTAINER" psql -U "$PG_USER" -d "$PG_DB" --quiet > /dev/null 2>&1

    echo "  PostgreSQL restored"
else
    echo "  WARN: No postgres.sql.gz found in backup, skipping DB restore"
fi

# ── 4. FalkorDB restore ──
if [ -d "${WORK_DIR}/falkordb" ]; then
    echo "  Restoring FalkorDB data..."
    mkdir -p "$FALKORDB_DATA"
    cp -r "${WORK_DIR}/falkordb"/* "$FALKORDB_DATA/" 2>/dev/null || true
    echo "  FalkorDB data restored"
else
    echo "  WARN: No falkordb/ directory in backup, skipping"
fi

# ── 5. Local storage restore ──
if [ -d "${WORK_DIR}/local-storage" ]; then
    echo "  Restoring local storage..."
    mkdir -p "$LOCAL_STORAGE"
    cp -r "${WORK_DIR}/local-storage"/* "$LOCAL_STORAGE/" 2>/dev/null || true
    echo "  Local storage restored"
else
    echo "  WARN: No local-storage/ directory in backup, skipping"
fi

# ── 6. Restart service ──
echo "  Restarting CogKOS service..."
if systemctl --user cat "$COGKOS_SERVICE" > /dev/null 2>&1; then
    systemctl --user start "$COGKOS_SERVICE"
    echo "  Service started via systemd"
else
    echo "  WARN: No systemd service found. Start CogKOS manually."
fi

# ── 7. Health check ──
echo "  Waiting for health check..."
RETRIES=10
while [ $RETRIES -gt 0 ]; do
    if curl -sf "$HEALTH_URL" > /dev/null 2>&1; then
        echo "  Health check passed"
        break
    fi
    RETRIES=$((RETRIES - 1))
    sleep 3
done

if [ $RETRIES -eq 0 ]; then
    echo "  WARN: Health check did not pass within 30 seconds"
    echo "  Verify manually: curl ${HEALTH_URL}"
fi

echo ""
echo "[$(date)] Restore complete from: ${BACKUP_FILE}"
