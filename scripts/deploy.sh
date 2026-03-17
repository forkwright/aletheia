#!/usr/bin/env bash
set -euo pipefail
# Deploy aletheia binary to the local instance.
# Usage: scripts/deploy.sh [--build] [--restart]
#   --build    Build release binary before deploying (default: use existing)
#   --restart  Restart systemd service after deploy (default: just copy)
#   No flags:  build + copy + restart (full deploy)

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTANCE_ROOT="${ALETHEIA_INSTANCE:-$HOME/ergon/instance}"
BINARY_SRC="$REPO_ROOT/target/release/aletheia"
BINARY_DST="${ALETHEIA_BINARY:-$HOME/ergon/bin/aletheia}"
SERVICE="aletheia.service"

# Parse flags
BUILD=false
RESTART=false
if [[ $# -eq 0 ]]; then
    BUILD=true
    RESTART=true
fi
while [[ $# -gt 0 ]]; do
    case "$1" in
        --build) BUILD=true; shift ;;
        --restart) RESTART=true; shift ;;
        *) echo "Unknown flag: $1"; exit 1 ;;
    esac
done

# Refresh OAuth token from Claude Code credentials
refresh_token() {
    local cred_file="$HOME/.claude/.credentials.json"
    if [[ -f "$cred_file" ]]; then
        local token
        token=$(python3 -c "import json; d=json.load(open('$cred_file')); print(d['claudeAiOauth']['accessToken'])" 2>/dev/null)
        if [[ -n "$token" ]]; then
            local svc_file="$HOME/.config/systemd/user/$SERVICE"
            if grep -q "ANTHROPIC_AUTH_TOKEN" "$svc_file" 2>/dev/null; then
                sed -i "s|ANTHROPIC_AUTH_TOKEN=.*|ANTHROPIC_AUTH_TOKEN=$token|" "$svc_file"
                echo "Token updated in $SERVICE"
            fi
        fi
    fi
}

# Build
if $BUILD; then
    echo "Building release binary..."
    cd "$REPO_ROOT"
    cargo build --release -p aletheia
    echo "Built: $(./target/release/aletheia --version)"
fi

# Verify binary exists
if [[ ! -f "$BINARY_SRC" ]]; then
    echo "Error: binary not found at $BINARY_SRC"
    echo "Run with --build or build manually first."
    exit 1
fi

# Stop service if running
if systemctl --user is-active "$SERVICE" &>/dev/null; then
    echo "Stopping $SERVICE..."
    systemctl --user stop "$SERVICE"
fi

# Copy binary
mkdir -p "$(dirname "$BINARY_DST")"
cp "$BINARY_SRC" "$BINARY_DST"
echo "Deployed: $BINARY_DST"

# Refresh token
refresh_token

# Restart
if $RESTART; then
    systemctl --user daemon-reload
    systemctl --user start "$SERVICE"
    sleep 3
    if systemctl --user is-active "$SERVICE" &>/dev/null; then
        echo "Service running."
        curl -sf http://localhost:18789/api/health 2>/dev/null | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'Health: {d[\"status\"]} v{d[\"version\"]}')" 2>/dev/null || echo "Health check: waiting..."
    else
        echo "ERROR: Service failed to start"
        journalctl --user -eu "$SERVICE" --since "10 sec ago" --no-pager | tail -5
        exit 1
    fi
fi
