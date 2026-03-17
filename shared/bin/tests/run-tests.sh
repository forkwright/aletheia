#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

if ! command -v bats &>/dev/null; then
    echo "error: bats not found. Install with: apt install bats" >&2
    exit 1
fi

echo "Running shared/bin utility script tests..."
echo ""

if [[ $# -gt 0 ]]; then
    bats "$@"
else
    bats "$SCRIPT_DIR"/*.bats
fi
