#!/usr/bin/env bash
# check.sh — Fast compile check (no dependency download, no codegen).
# Use this in CI gate before running full test suite.

set -euo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "=== cargo check (workspace) ==="
cd "$DIR"
cargo check --all --quiet

echo ""
echo "✅ Check passed"
