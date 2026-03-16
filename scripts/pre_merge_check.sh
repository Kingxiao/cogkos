#!/bin/bash
# Pre-merge check script - run before pushing to main

set -e

echo "=== Pre-Merge Check ==="

echo "[1/6] Running cargo check..."
cargo check --all-targets

echo "[2/6] Running cargo test..."
cargo test --all-targets

echo "[3/6] Running cargo clippy..."
cargo clippy --all-targets -- -D warnings || true

echo "[4/6] Checking cargo fmt..."
cargo fmt --check

echo "[5/6] Checking for TODO/FIXME..."
TODO_COUNT=$(grep -r "TODO\|FIXME" crates/ --include="*.rs" 2>/dev/null | wc -l)
if [ "$TODO_COUNT" -gt 0 ]; then
    echo "⚠️  Found $TODO_COUNT TODO/FIXME markers:"
    grep -r "TODO\|FIXME" crates/ --include="*.rs" | head -10
fi

echo "[6/6] Checking for incomplete implementations..."
INCOMPLETE=$(grep -rn "unimplemented!\|panic!(\"TODO" crates/ --include="*.rs" 2>/dev/null | wc -l)
if [ "$INCOMPLETE" -gt 0 ]; then
    echo "⚠️  Found $INCOMPLETE incomplete implementations:"
    grep -rn "unimplemented!\|panic!(\"TODO" crates/ --include="*.rs" | head -10
fi

echo "=== Pre-Merge Check Complete ==="
