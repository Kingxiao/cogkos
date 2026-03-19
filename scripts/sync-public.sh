#!/bin/bash
# Sync code to public repo (excluding sensitive files)
set -e

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

git clone --depth 1 https://github.com/Kingxiao/cogkos.git "$TMPDIR/cogkos" 2>/dev/null

# Exclude sensitive files and directories
rsync -av --delete \
  --exclude='.git' \
  --exclude='.env' \
  --exclude='.claude/' \
  --exclude='CLAUDE.md' \
  --exclude='data/' \
  --exclude='backups/' \
  --exclude='target/' \
  --exclude='docs/archive/' \
  --exclude='docs/test-plan.md' \
  --exclude='scripts/cogkos-bg.sh' \
  /home/zichuan/.openclaw/workspace/cogkos-archive/ "$TMPDIR/cogkos/" > /dev/null 2>&1

cd "$TMPDIR/cogkos"
git config user.name "Kingxiao"
git config user.email "mushiqingqian@gmail.com"
git add -A

if git diff --cached --quiet; then
  echo "No changes to push"
  exit 0
fi

git diff --cached --stat | tail -5
MSG="${1:-sync: update from private repo}"
git commit -m "$MSG"
git push origin main 2>&1 | tail -3
echo "Synced to public repo"
