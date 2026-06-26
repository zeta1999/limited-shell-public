#!/usr/bin/env bash
# test.sh — Run the full test suite.
# Exit 0 on success, non-zero on failure.

set -euo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "=== Running tests ==="
cd "$DIR"
cargo test --all -- --nocapture

echo ""
echo "✅ All tests passed"
