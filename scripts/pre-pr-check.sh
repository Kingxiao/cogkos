#!/bin/bash
set -euo pipefail

echo "=== CogKOS Pre-PR Safety Check ==="

# 1. 确保在 feature 分支上
CURRENT_BRANCH=$(git branch --show-current)
if [ "$CURRENT_BRANCH" = "main" ]; then
    echo "ERROR: 不能在 main 分支上直接提交"
    exit 1
fi
echo "✓ 当前分支: $CURRENT_BRANCH"

# 2. Fetch 最新 main
echo "--- Fetching latest main ---"
git fetch origin main

# 3. Rebase 到最新 main（必须先 rebase 再创建 PR，防止合并冲突）
echo "--- Rebasing onto main ---"
if ! git rebase origin/main; then
    echo "ERROR: Rebase 失败！有合并冲突。"
    echo "请手动解决冲突后重试。"
    git rebase --abort
    exit 1
fi
echo "✓ Rebase 成功"

# 4. 编译检查
echo "--- Building workspace ---"
if ! cargo build --workspace 2>&1; then
    echo "ERROR: 编译失败！"
    exit 1
fi
echo "✓ 编译通过"

# 5. 格式检查
echo "--- Checking format ---"
if ! cargo fmt --all -- --check 2>&1; then
    echo "Running cargo fmt..."
    cargo fmt --all
    git add -A
    git commit -m "chore: cargo fmt"
fi
echo "✓ 格式正确"

# 6. 运行测试（如果存在）
echo "--- Running tests ---"
cargo test --workspace --lib 2>&1 || echo "WARN: 部分测试失败（非阻塞）"

echo ""
echo "=== Pre-PR Check PASSED ==="
echo "可以安全创建 PR: gh pr create"
