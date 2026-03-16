#!/usr/bin/env bash
# CogKOS backup script — PostgreSQL + SeaweedFS
# Usage: ./scripts/backup.sh [backup_dir]
# Cron:  0 2 * * * /path/to/cogkos/scripts/backup.sh /backups/cogkos

set -euo pipefail

BACKUP_DIR="${1:-/tmp/cogkos-backup}"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_PATH="${BACKUP_DIR}/${TIMESTAMP}"
RETENTION_DAYS="${BACKUP_RETENTION_DAYS:-30}"

# Database config (from env or defaults)
DB_HOST="${DATABASE_HOST:-localhost}"
DB_PORT="${DATABASE_PORT:-5432}"
DB_NAME="${DATABASE_NAME:-cogkos}"
DB_USER="${DATABASE_USER:-cogkos}"

# SeaweedFS config
SEAWEEDFS_MASTER="${SEAWEEDFS_MASTER:-localhost:9333}"
SEAWEEDFS_FILER="${SEAWEEDFS_FILER:-localhost:8888}"

mkdir -p "${BACKUP_PATH}"

echo "[$(date)] Starting CogKOS backup to ${BACKUP_PATH}"

# ── PostgreSQL backup ──
echo "[$(date)] Backing up PostgreSQL..."
pg_dump \
    -h "${DB_HOST}" \
    -p "${DB_PORT}" \
    -U "${DB_USER}" \
    -d "${DB_NAME}" \
    --format=custom \
    --compress=9 \
    --file="${BACKUP_PATH}/postgres.dump"

PG_SIZE=$(du -sh "${BACKUP_PATH}/postgres.dump" | cut -f1)
echo "[$(date)] PostgreSQL backup complete: ${PG_SIZE}"

# ── SeaweedFS backup (via filer export) ──
echo "[$(date)] Backing up SeaweedFS..."
if command -v weed &>/dev/null; then
    weed shell -master="${SEAWEEDFS_MASTER}" <<CMD
fs.cd /
fs.ls
CMD
    # Export bucket contents via S3 sync
    if command -v aws &>/dev/null; then
        aws s3 sync \
            --endpoint-url "http://${SEAWEEDFS_FILER%%:*}:8333" \
            s3://cogkos-documents \
            "${BACKUP_PATH}/seaweedfs/" \
            --no-sign-request 2>/dev/null || \
        aws s3 sync \
            --endpoint-url "http://${SEAWEEDFS_FILER%%:*}:8333" \
            s3://cogkos-documents \
            "${BACKUP_PATH}/seaweedfs/"
    else
        echo "[$(date)] WARN: aws CLI not found, skipping SeaweedFS backup"
    fi
else
    echo "[$(date)] WARN: weed CLI not found, attempting S3 sync only"
    if command -v aws &>/dev/null; then
        aws s3 sync \
            --endpoint-url "http://${SEAWEEDFS_FILER%%:*}:8333" \
            s3://cogkos-documents \
            "${BACKUP_PATH}/seaweedfs/" 2>/dev/null || \
        echo "[$(date)] WARN: SeaweedFS S3 sync failed"
    fi
fi

# ── Cleanup old backups ──
echo "[$(date)] Cleaning backups older than ${RETENTION_DAYS} days..."
find "${BACKUP_DIR}" -maxdepth 1 -type d -mtime "+${RETENTION_DAYS}" -exec rm -rf {} \; 2>/dev/null || true

# ── Summary ──
TOTAL_SIZE=$(du -sh "${BACKUP_PATH}" | cut -f1)
echo "[$(date)] Backup complete: ${BACKUP_PATH} (${TOTAL_SIZE})"
echo "[$(date)] Contents:"
ls -lh "${BACKUP_PATH}/"
