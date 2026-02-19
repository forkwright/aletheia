#!/usr/bin/env bash
# Run tests for a specific module or set of changed files
# Usage: ./scripts/test-module.sh [module-path-pattern]
# Examples:
#   ./scripts/test-module.sh organon/built-in   # all built-in tool tests
#   ./scripts/test-module.sh nous/pipeline       # pipeline tests
#   ./scripts/test-module.sh                     # tests for files changed since HEAD~1

set -euo pipefail
cd "$(dirname "$0")/.."

if [ $# -eq 0 ]; then
  echo "Running tests for changed files (vs HEAD~1)..."
  npx vitest run --changed HEAD~1 --reporter dot 2>&1
else
  pattern="$1"
  echo "Running tests matching: $pattern"
  npx vitest run --reporter dot "$pattern" 2>&1
fi
