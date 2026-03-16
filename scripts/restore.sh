#!/usr/bin/env bash
# CogKOS restore script — PostgreSQL + SeaweedFS
# Usage: ./scripts/restore.sh /backups/cogkos/20260316_020000

set -euo pipefail

BACKUP_PATH="${1:?Usage: $0 <backup_path>}"

# Database config
DB_HOST="${DATABASE_HOST:-localhost}"
DB_PORT="${DATABASE_PORT:-5432}"
DB_NAME="${DATABASE_NAME:-cogkos}"
DB_USER="${DATABASE_USER:-cogkos}"

if [ ! -d "${BACKUP_PATH}" ]; then
    echo "ERROR: Backup path not found: ${BACKUP_PATH}"
    exit 1
fi

echo "[$(date)] Starting CogKOS restore from ${BACKUP_PATH}"
echo "WARNING: This will overwrite the current database. Press Ctrl+C to abort."
sleep 5

# ── PostgreSQL restore ──
if [ -f "${BACKUP_PATH}/postgres.dump" ]; then
    echo "[$(date)] Restoring PostgreSQL..."

    # Drop and recreate database
    psql -h "${DB_HOST}" -p "${DB_PORT}" -U "${DB_USER}" -d postgres \
        -c "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '${DB_NAME}' AND pid <> pg_backend_pid();" 2>/dev/null || true
    psql -h "${DB_HOST}" -p "${DB_PORT}" -U "${DB_USER}" -d postgres \
        -c "DROP DATABASE IF EXISTS ${DB_NAME};"
    psql -h "${DB_HOST}" -p "${DB_PORT}" -U "${DB_USER}" -d postgres \
        -c "CREATE DATABASE ${DB_NAME};"

    pg_restore \
        -h "${DB_HOST}" \
        -p "${DB_PORT}" \
        -U "${DB_USER}" \
        -d "${DB_NAME}" \
        --no-owner \
        --no-privileges \
        "${BACKUP_PATH}/postgres.dump"

    echo "[$(date)] PostgreSQL restore complete"
else
    echo "[$(date)] WARN: No postgres.dump found, skipping DB restore"
fi

# ── SeaweedFS restore ──
SEAWEEDFS_S3="${SEAWEEDFS_S3_ENDPOINT:-http://localhost:8333}"
if [ -d "${BACKUP_PATH}/seaweedfs" ]; then
    echo "[$(date)] Restoring SeaweedFS documents..."
    if command -v aws &>/dev/null; then
        aws s3 sync \
            "${BACKUP_PATH}/seaweedfs/" \
            s3://cogkos-documents \
            --endpoint-url "${SEAWEEDFS_S3}"
        echo "[$(date)] SeaweedFS restore complete"
    else
        echo "[$(date)] WARN: aws CLI not found, skipping SeaweedFS restore"
    fi
else
    echo "[$(date)] WARN: No seaweedfs/ directory found, skipping object restore"
fi

echo "[$(date)] Restore complete from ${BACKUP_PATH}"
