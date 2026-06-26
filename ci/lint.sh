#!/usr/bin/env bash
# lint.sh — Run clippy lints and format checks.
# Exit 0 on success, non-zero on failure.

set -euo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "=== Clippy (all crates, warnings as errors) ==="
cd "$DIR"
cargo clippy --all --all-targets -- -D warnings

echo ""
echo "=== cargo fmt check ==="
cargo fmt --all --check

echo ""
echo "✅ All lint checks passed"
