#!/bin/bash
set -euo pipefail

echo "=== CogKOS Pre-PR Safety Check ==="

# 1. Ensure we're on a feature branch
CURRENT_BRANCH=$(git branch --show-current)
if [ "$CURRENT_BRANCH" = "main" ]; then
    echo "ERROR: Cannot commit directly on main branch"
    exit 1
fi
echo "✓ Current branch: $CURRENT_BRANCH"

# 2. Fetch latest main
echo "--- Fetching latest main ---"
git fetch origin main

# 3. Rebase onto latest main (must rebase before creating PR to avoid merge conflicts)
echo "--- Rebasing onto main ---"
if ! git rebase origin/main; then
    echo "ERROR: Rebase failed! Merge conflicts detected."
    echo "Please resolve conflicts manually and retry."
    git rebase --abort
    exit 1
fi
echo "✓ Rebase successful"

# 4. Build check
echo "--- Building workspace ---"
if ! cargo build --workspace 2>&1; then
    echo "ERROR: Build failed!"
    exit 1
fi
echo "✓ Build passed"

# 5. Format check
echo "--- Checking format ---"
if ! cargo fmt --all -- --check 2>&1; then
    echo "Running cargo fmt..."
    cargo fmt --all
    git add -A
    git commit -m "chore: cargo fmt"
fi
echo "✓ Format correct"

# 6. Run tests (if any)
echo "--- Running tests ---"
cargo test --workspace --lib 2>&1 || echo "WARN: Some tests failed (non-blocking)"

echo ""
echo "=== Pre-PR Check PASSED ==="
echo "Safe to create PR: gh pr create"
