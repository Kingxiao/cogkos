#!/bin/bash
# CogKOS database backup script
# Crontab: 0 3 * * * /home/zichuan/.openclaw/workspace/cogkos-archive/scripts/backup.sh

set -euo pipefail

BACKUP_DIR="/home/zichuan/.openclaw/workspace/cogkos-archive/backups"
RETENTION_DAYS=7
PG_CONTAINER="cogkos-postgres"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

mkdir -p "$BACKUP_DIR"

echo "[$(date)] Starting CogKOS backup..."

# PostgreSQL backup
PG_BACKUP="$BACKUP_DIR/cogkos_pg_${TIMESTAMP}.sql.gz"
if docker exec "$PG_CONTAINER" pg_dump -U cogkos -d cogkos | gzip > "$PG_BACKUP"; then
    echo "  ✅ PostgreSQL: $PG_BACKUP ($(du -h "$PG_BACKUP" | cut -f1))"
else
    echo "  ❌ PostgreSQL backup FAILED"; exit 1
fi

# FalkorDB backup
FDB_BACKUP="$BACKUP_DIR/cogkos_falkordb_${TIMESTAMP}.rdb"
docker exec cogkos-falkordb redis-cli BGSAVE > /dev/null 2>&1; sleep 2
if docker cp cogkos-falkordb:/data/dump.rdb "$FDB_BACKUP" 2>/dev/null; then
    echo "  ✅ FalkorDB: $FDB_BACKUP ($(du -h "$FDB_BACKUP" | cut -f1))"
else
    echo "  ⚠️  FalkorDB backup skipped"
fi

# Cleanup
DELETED=$(find "$BACKUP_DIR" -name "cogkos_*" -mtime +${RETENTION_DAYS} -delete -print | wc -l)
echo "  🗑️  Cleaned $DELETED old backups (retention: ${RETENTION_DAYS}d)"
echo "[$(date)] Done."
