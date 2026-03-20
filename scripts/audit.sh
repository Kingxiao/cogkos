#!/usr/bin/env bash
# CogKOS configuration audit — catches cross-file inconsistencies that unit tests miss.
# Run: bash scripts/audit.sh
# CI: runs as part of the audit job in .github/workflows/ci.yml

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FAIL=0

pass() { echo "  ✓ $1"; }
fail() { echo "  ✗ $1"; FAIL=1; }
section() { echo ""; echo "[$1]"; }

# ─────────────────────────────────────────────────────
# Check 1: Environment variable completeness
# Every std::env::var("X") in Rust source should be declared in .env.example
# ─────────────────────────────────────────────────────
section "Check 1: Environment variable completeness"

# Whitelist: system/build/test vars that don't belong in .env.example
ENV_WHITELIST='RUST_LOG|PATH|HOME|USER|CARGO_|SQLX_|TEST_|AWS_DEFAULT_REGION|AWS_REGION|AWS_ACCESS_KEY_ID|AWS_SECRET_ACCESS_KEY|OTEL_|FORCE_COLOR|NO_COLOR|TERM'

# Extract env var names from Rust source
CODE_VARS=$(grep -rhoP 'std::env::var\("([A-Z_][A-Z0-9_]*)"\)' "$ROOT/src/" "$ROOT/crates/" 2>/dev/null \
    | grep -oP '"[A-Z_][A-Z0-9_]*"' | tr -d '"' | sort -u)

# Extract declared vars from .env.example (active or commented-out VAR_NAME=)
DECLARED_VARS=$(grep -oP '^#?\s*[A-Z_][A-Z0-9_]*(?==)' "$ROOT/.env.example" 2>/dev/null \
    | grep -oP '[A-Z_][A-Z0-9_]*' | sort -u)

MISSING_VARS=()
for var in $CODE_VARS; do
    # Skip whitelisted
    if echo "$var" | grep -qE "^($ENV_WHITELIST)"; then continue; fi
    if ! echo "$DECLARED_VARS" | grep -qx "$var"; then
        MISSING_VARS+=("$var")
    fi
done

if [ ${#MISSING_VARS[@]} -eq 0 ]; then
    pass "All code env vars declared in .env.example"
else
    fail "Env vars read in code but missing from .env.example:"
    for v in "${MISSING_VARS[@]}"; do
        echo "      - $v"
    done
fi

# ─────────────────────────────────────────────────────
# Check 2: Port number consistency
# ─────────────────────────────────────────────────────
section "Check 2: Port number consistency"

check_port() {
    local name="$1" expected="$2"
    shift 2
    local ok=true
    for pair in "$@"; do
        local file="${pair%%:*}" pattern="${pair##*:}"
        if [ -f "$ROOT/$file" ]; then
            if ! grep -q "$pattern" "$ROOT/$file" 2>/dev/null; then
                fail "$name: expected $expected in $file (pattern: $pattern)"
                ok=false
            fi
        fi
    done
    if $ok; then pass "$name port = $expected consistent"; fi
}

# MCP port: 3000
check_port "MCP" "3000" \
    "docker-compose.yml:3000:3000" \
    ".env.example:MCP_PORT=3000" \
    "config/default.toml:port = 3000" \
    "Dockerfile:EXPOSE 3000"

# Health port: 8081
check_port "Health" "8081" \
    "docker-compose.yml:8081:8081" \
    ".env.example:HEALTH_PORT=8081" \
    "Dockerfile:EXPOSE.*8081"

# PostgreSQL host port: 5435 (offset to avoid conflict with local PG)
check_port "PostgreSQL" "5435" \
    "docker-compose.yml:5435:5432" \
    ".env.example:localhost:5435" \
    "config/default.toml:localhost:5435"

# FalkorDB host port: 6381 (offset to avoid conflict with local Redis)
check_port "FalkorDB" "6381" \
    "docker-compose.yml:6381:6379" \
    ".env.example:localhost:6381"

# ─────────────────────────────────────────────────────
# Check 3: Chinese text in public-repo files
# Scans non-docs source files for CJK characters
# ─────────────────────────────────────────────────────
section "Check 3: Chinese text in public-repo files"

# Must use relative path — absolute paths break --include='Dockerfile*' glob matching
CJK_HITS=$(cd "$ROOT" && grep -rnP '[\x{4e00}-\x{9fff}]' \
    --include='*.rs' --include='*.yml' --include='*.yaml' \
    --include='*.toml' --include='*.sh' --include='Dockerfile*' \
    . 2>/dev/null \
    | grep -v '/docs/' \
    | grep -v 'CLAUDE.md' \
    | grep -v '\.claude/' \
    | grep -v '/target/' \
    | grep -v 'archive/' \
    | grep -v '/data/' \
    | grep -v 'classifier\.rs' \
    | grep -v 'deep_classifier\.rs' \
    | grep -v 'conflict\.rs' \
    | grep -v 'helpers\.rs' \
    | grep -v 'query\.rs' \
    | grep -v 'submit_query_flow_test\.rs' \
    || true)

if [ -z "$CJK_HITS" ]; then
    pass "No CJK characters in source/config files"
else
    fail "CJK characters found in public-repo files:"
    echo "$CJK_HITS" | head -20 | while IFS= read -r line; do
        echo "      $line"
    done
    COUNT=$(echo "$CJK_HITS" | wc -l)
    if [ "$COUNT" -gt 20 ]; then echo "      ... and $((COUNT - 20)) more"; fi
fi

# ─────────────────────────────────────────────────────
# Check 4: MCP schema-struct alignment
# Verifies tool_schemas.rs required fields exist in types.rs structs
# ─────────────────────────────────────────────────────
section "Check 4: MCP schema-struct alignment"

SCHEMA_FILE="$ROOT/crates/cogkos-mcp/src/server/tool_schemas.rs"
TYPES_FILE="$ROOT/crates/cogkos-mcp/src/tools/types.rs"

if [ -f "$SCHEMA_FILE" ] && [ -f "$TYPES_FILE" ]; then
    # Extract required fields from schema: lines containing "required" key with json array
    # Pattern: "required".to_string(), serde_json::json!(["field1", "field2", ...])
    SCHEMA_REQUIRED=$(grep '"required"' "$SCHEMA_FILE" \
        | grep -oP 'json!\(\[([^\]]+)\]' \
        | grep -oP '"[a-z_0-9]+"' | tr -d '"' | sort -u || true)

    # Extract struct fields from types.rs (pub field_name:)
    TYPES_FIELDS=$(grep -oP 'pub\s+([a-z_]+)\s*:' "$TYPES_FILE" \
        | grep -oP '[a-z_]+(?=\s*:)' | sort -u)

    # Also check serde renames (e.g. content_base64)
    SERDE_RENAMES=$(grep -oP 'rename\s*=\s*"([a-z0-9_]+)"' "$TYPES_FILE" \
        | grep -oP '"[a-z0-9_]+"' | tr -d '"' | sort -u || true)

    ALL_FIELDS=$(echo -e "$TYPES_FIELDS\n$SERDE_RENAMES" | sort -u)

    SCHEMA_MISSING=()
    for field in $SCHEMA_REQUIRED; do
        # Skip api_key (handled via headers) and type (serde tag, not a field)
        if [ "$field" = "api_key" ] || [ "$field" = "type" ]; then continue; fi
        if ! echo "$ALL_FIELDS" | grep -qx "$field"; then
            SCHEMA_MISSING+=("$field")
        fi
    done

    if [ ${#SCHEMA_MISSING[@]} -eq 0 ]; then
        pass "All schema required fields exist in types.rs structs"
    else
        fail "Schema required fields missing from types.rs:"
        for f in "${SCHEMA_MISSING[@]}"; do echo "      - $f"; done
    fi

    # Check list_subscriptions type enum variants
    # The schema declares: "enum": ["rss", "webhook", "api"]
    # Extract from the list_subscriptions section (last tool in the file)
    LIST_SCHEMA_ENUMS=$(sed -n '/list_subscriptions/,/tools.push/p' "$SCHEMA_FILE" \
        | grep -oP '"enum".*?\[([^\]]+)\]' \
        | grep -oP '"[a-z_]+"' | tr -d '"' | grep -v '^enum$' | sort -u || true)
    if [ -n "$LIST_SCHEMA_ENUMS" ]; then
        LIST_ENUM=$(grep -A5 'enum ListSubscriptionsRequest' "$TYPES_FILE" \
            | grep -oP '^\s+([A-Z][a-z]+)' | awk '{print tolower($1)}' | sort -u || true)
        ENUM_MISSING=()
        for v in $LIST_SCHEMA_ENUMS; do
            if ! echo "$LIST_ENUM" | grep -qx "$v"; then
                ENUM_MISSING+=("$v")
            fi
        done
        if [ ${#ENUM_MISSING[@]} -eq 0 ]; then
            pass "ListSubscriptions enum variants match Rust enum"
        else
            fail "ListSubscriptions enum variants missing from Rust enum:"
            for v in "${ENUM_MISSING[@]}"; do echo "      - $v"; done
        fi
    fi
else
    fail "Schema or types file not found"
fi

# ─────────────────────────────────────────────────────
# Check 5: Sensitive file leak detection
# ─────────────────────────────────────────────────────
section "Check 5: Sensitive file leak detection"

# CLAUDE.md is tracked in private repo (excluded by sync-public.sh), so not listed here
SENSITIVE_PATTERNS=('.env' '*.key' '*.pem' 'credentials.json' 'service-account*.json')

LEAKED=()
for pattern in "${SENSITIVE_PATTERNS[@]}"; do
    MATCHES=$(git -C "$ROOT" ls-files "$pattern" 2>/dev/null || true)
    if [ -n "$MATCHES" ]; then
        while IFS= read -r f; do
            # Allow .env.example and .env.test
            if [[ "$f" == ".env.example" ]] || [[ "$f" == ".env.test" ]]; then continue; fi
            LEAKED+=("$f")
        done <<< "$MATCHES"
    fi
done

if [ ${#LEAKED[@]} -eq 0 ]; then
    pass "No sensitive files tracked by git"
else
    fail "Sensitive files tracked by git:"
    for f in "${LEAKED[@]}"; do echo "      - $f"; done
fi

# ─────────────────────────────────────────────────────
# Check 6: Version consistency
# ─────────────────────────────────────────────────────
section "Check 6: Version consistency"

# Rust version: Dockerfile vs CI
DOCKER_RUST=$(grep -oP 'FROM rust:\K[0-9.]+' "$ROOT/Dockerfile" 2>/dev/null || echo "?")
# CI may use RUST_VERSION env var or dtolnay/rust-toolchain@VERSION
CI_RUST=$(grep -oP 'RUST_VERSION:\s*"\K[0-9.]+' "$ROOT/.github/workflows/ci.yml" 2>/dev/null \
    || grep -oP 'rust-toolchain@\K[0-9.]+' "$ROOT/.github/workflows/ci.yml" 2>/dev/null | head -1 \
    || echo "?")
# Compare major.minor only (1.94 == 1.94.0)
DOCKER_RUST_MM=$(echo "$DOCKER_RUST" | grep -oP '^[0-9]+\.[0-9]+')
CI_RUST_MM=$(echo "$CI_RUST" | grep -oP '^[0-9]+\.[0-9]+')
if [ "$DOCKER_RUST_MM" = "$CI_RUST_MM" ]; then
    pass "Rust version consistent: $DOCKER_RUST_MM (Dockerfile=$DOCKER_RUST, CI=$CI_RUST)"
else
    fail "Rust version mismatch: Dockerfile=$DOCKER_RUST, CI=$CI_RUST"
fi

# PostgreSQL version: docker-compose vs docker-compose.test
DC_PG=$(grep -oP 'pgvector/pgvector:pg\K[0-9]+' "$ROOT/docker-compose.yml" 2>/dev/null || echo "?")
TEST_PG=$(grep -oP 'pgvector/pgvector:pg\K[0-9]+' "$ROOT/docker-compose.test.yml" 2>/dev/null || echo "?")
if [ "$DC_PG" = "$TEST_PG" ]; then
    pass "PostgreSQL version consistent: pg$DC_PG (dev = test)"
else
    fail "PostgreSQL version mismatch: dev=pg$DC_PG, test=pg$TEST_PG"
fi

# ─────────────────────────────────────────────────────
# Check 7: Deprecated reference detection
# ─────────────────────────────────────────────────────
section "Check 7: Deprecated reference detection"

DEPRECATED_TERMS=("X-Tenant-ID" "port.*9090")

for term in "${DEPRECATED_TERMS[@]}"; do
    HITS=$(grep -rnP "$term" "$ROOT" \
        --include='*.rs' --include='*.yml' --include='*.yaml' \
        --include='*.toml' --include='*.sh' --include='*.md' \
        --include='Dockerfile*' \
        2>/dev/null \
        | grep -v '/target/' \
        | grep -v 'archive/' \
        | grep -v '\.claude/' \
        | grep -v 'CLAUDE.md' \
        | grep -v 'audit\.sh' \
        || true)

    if [ -z "$HITS" ]; then
        pass "No references to deprecated: $term"
    else
        fail "Deprecated reference '$term' found:"
        echo "$HITS" | head -5 | while IFS= read -r line; do
            echo "      $line"
        done
    fi
done

# ─────────────────────────────────────────────────────
# Summary
# ─────────────────────────────────────────────────────
echo ""
echo "─────────────────────────────────────────"
if [ $FAIL -eq 0 ]; then
    echo "✓ All audit checks passed"
    exit 0
else
    echo "✗ Some audit checks failed — fix before merging"
    exit 1
fi
