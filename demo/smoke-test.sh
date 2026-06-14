#!/usr/bin/env bash
# Smoke test for the Aletheia demo instance.
# Verifies that the server starts, health endpoint responds, and the demo agent exists.
#
# Usage:
#   demo/smoke-test.sh [--instance-root <path>] [--port <port>] [--timeout <secs>]
#
# Prerequisites: aletheia binary on PATH, curl, jq.
# The server must not already be running on the demo port.
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
INSTANCE_ROOT="${INSTANCE_ROOT:-$SCRIPT_DIR/instance}"
PORT=18799
BASE_URL="http://127.0.0.1:${PORT}"
HEALTH_URL="${BASE_URL}/api/health"
AGENTS_URL="${BASE_URL}/api/v1/nous"
TIMEOUT=30
SERVER_PID=""

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
ok()   { printf 'OK:   %s\n' "$*"; }

cleanup() {
    if [[ -n "$SERVER_PID" ]]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

while [[ $# -gt 0 ]]; do
    case "$1" in
        --instance-root) INSTANCE_ROOT="$2"; shift 2 ;;
        --port)          PORT="$2"; BASE_URL="http://127.0.0.1:${PORT}"; HEALTH_URL="${BASE_URL}/api/health"; AGENTS_URL="${BASE_URL}/api/v1/nous"; shift 2 ;;
        --timeout)       TIMEOUT="$2"; shift 2 ;;
        *) fail "unknown flag: $1" ;;
    esac
done

if ! command -v aletheia &>/dev/null; then
    fail "aletheia binary not found on PATH. Build with: cargo build --release && cp target/release/aletheia ~/.local/bin/"
fi
if ! command -v curl &>/dev/null; then
    fail "curl not found"
fi
if ! command -v jq &>/dev/null; then
    fail "jq not found"
fi

# 1. Check config
printf '--- config check ---\n'
if ! aletheia -r "$INSTANCE_ROOT" check-config; then
    fail "config check failed"
fi
ok "config valid at $INSTANCE_ROOT"

# 2. Start the server in background
printf '--- starting server ---\n'
aletheia -r "$INSTANCE_ROOT" serve &
SERVER_PID="$!"

# 3. Wait for health endpoint
printf '--- waiting for health ---\n'
elapsed=0
while (( elapsed < TIMEOUT )); do
    if response=$(curl -sf --max-time 3 "$HEALTH_URL" 2>/dev/null); then
        status=$(printf '%s' "$response" | jq -r '.status // empty' 2>/dev/null || true)
        if [[ "$status" == "healthy" || "$status" == "degraded" ]]; then
            ok "server healthy (status=$status)"
            break
        fi
    fi
    sleep 2
    elapsed=$(( elapsed + 2 ))
done
if (( elapsed >= TIMEOUT )); then
    fail "server did not become healthy within ${TIMEOUT}s"
fi

# 4. Verify demo agent is registered
printf '--- verifying demo agent ---\n'
agents_response=$(curl -sf --max-time 5 \
    -H "X-Requested-With: aletheia" \
    "$AGENTS_URL" 2>/dev/null) || fail "could not reach agents endpoint"
agent_count=$(printf '%s' "$agents_response" | jq '[.items // . | .[] | select(.id == "demo")] | length' 2>/dev/null || echo 0)
if [[ "$agent_count" -lt 1 ]]; then
    fail "demo agent not found in response: ${agents_response:0:200}"
fi
ok "demo agent registered"

printf '\n--- PASS ---\n'
printf 'Demo instance is working. Open the TUI to try it:\n'
printf '  aletheia -r %s tui\n' "$INSTANCE_ROOT"
