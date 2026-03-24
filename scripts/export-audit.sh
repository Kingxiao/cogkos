#!/usr/bin/env bash
# Export CogKOS audit logs for compliance
# Usage: bash scripts/export-audit.sh [--since 2026-01-01] [--format csv|json] [--tenant ID] [output_file]
set -euo pipefail

SINCE="" FORMAT="csv" TENANT="" OUTPUT=""
PG_CONTAINER="${PG_CONTAINER:-cogkos-postgres}"
PG_USER="${PG_USER:-cogkos}" PG_DB="${PG_DB:-cogkos}"
DATABASE_URL="${DATABASE_URL:-}"

show_help() { cat <<'EOF'
CogKOS audit log export

Usage: bash scripts/export-audit.sh [OPTIONS] [output_file]

Options:
  --since DATE    Filter from date (ISO 8601, e.g. 2026-01-01)
  --format FMT   csv (default) or json
  --tenant ID     Filter by tenant_id
  -h, --help      Show this help

Environment variables:
  DATABASE_URL    Direct PostgreSQL connection (bypasses docker exec)
  PG_CONTAINER    Docker container (default: cogkos-postgres)
  PG_USER / PG_DB PostgreSQL user/database (default: cogkos)

Tables (preference order): audit_logs > epistemic_claims + agent_feedbacks
EOF
exit 0; }

while [[ $# -gt 0 ]]; do case "$1" in
    -h|--help) show_help ;; --since) SINCE="$2"; shift 2 ;;
    --format) FORMAT="$2"; shift 2 ;; --tenant) TENANT="$2"; shift 2 ;;
    *) OUTPUT="$1"; shift ;; esac; done
[[ "$FORMAT" == "csv" || "$FORMAT" == "json" ]] || { echo "ERROR: --format must be csv or json" >&2; exit 1; }

# Build WHERE clause
W=()
[[ -n "$SINCE" ]] && W+=("timestamp >= '${SINCE}'::timestamptz")
[[ -n "$TENANT" ]] && W+=("tenant_id = '${TENANT}'")
WHERE=""; [[ ${#W[@]} -gt 0 ]] && WHERE="WHERE $(IFS=' AND '; echo "${W[*]}")"

AUDIT_SQL="SELECT id::text, timestamp::text, action, category, severity, \
COALESCE(tenant_id,'') AS tenant_id, COALESCE(actor_user_id,actor_service_id,'') AS actor, \
COALESCE(target_resource_type,'')||':'||COALESCE(target_resource_id,'') AS target, outcome \
FROM audit_logs ${WHERE} ORDER BY timestamp DESC"

FALLBACK_SQL="SELECT * FROM ( \
SELECT id::text, created_at::text AS timestamp, 'create_claim' AS action, 'Knowledge' AS category, \
'Info' AS severity, tenant_id, COALESCE(claimant->>'agent_id',claimant->>'user_id','') AS actor, \
'claim:'||id::text AS target, 'Success' AS outcome FROM epistemic_claims ${WHERE} \
UNION ALL SELECT id::text, created_at::text AS timestamp, 'submit_feedback' AS action, \
'Feedback' AS category, 'Info' AS severity, COALESCE(tenant_id,'') AS tenant_id, \
COALESCE(agent_id,'') AS actor, 'feedback:'||id::text AS target, \
CASE WHEN success THEN 'Success' ELSE 'Failure' END AS outcome \
FROM agent_feedbacks ${WHERE}) combined ORDER BY timestamp DESC"

run_psql() {
    local sql="$1" flags="${2:-}"
    if [[ -n "$DATABASE_URL" ]]; then
        psql "$DATABASE_URL" -t -A $flags -c "$sql" 2>/dev/null
    else
        docker exec "$PG_CONTAINER" psql -U "$PG_USER" -d "$PG_DB" -t -A $flags -c "$sql" 2>/dev/null
    fi
}

HAS_AUDIT=$(run_psql "SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_name='audit_logs')" || echo "f")
if [[ "$HAS_AUDIT" == *"t"* ]]; then SQL="$AUDIT_SQL"
else echo "INFO: audit_logs not found, using claims+feedbacks" >&2; SQL="$FALLBACK_SQL"; fi

execute() {
    if [[ "$FORMAT" == "csv" ]]; then
        echo "id,timestamp,action,category,severity,tenant_id,actor,target,outcome"
        run_psql "$SQL" "-F,"
    else
        echo "["
        local first=true
        run_psql "$SQL" "-F," | while IFS=',' read -r id ts act cat sev tid actor tgt out; do
            $first && first=false || echo ","
            printf '  {"id":"%s","timestamp":"%s","action":"%s","category":"%s","severity":"%s","tenant_id":"%s","actor":"%s","target":"%s","outcome":"%s"}' \
                "$id" "$ts" "$act" "$cat" "$sev" "$tid" "$actor" "$tgt" "$out"
        done; echo ""; echo "]"
    fi
}

if [[ -z "$OUTPUT" || "$OUTPUT" == "-" ]]; then execute
else execute > "$OUTPUT"; echo "Exported to ${OUTPUT} ($(wc -l < "$OUTPUT") lines, ${FORMAT})" >&2; fi
