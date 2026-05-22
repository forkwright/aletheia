#!/usr/bin/env bash
set -euo pipefail

# Smoke-test proskenion startup against a live or script-started Aletheia server.
#
# This is intentionally a process/display/connectivity smoke, not a GUI driver.
#
# Usage:
#   scripts/smoke-proskenion.sh [--server-url URL] [--auth-token TOKEN]
#                              [--proskenion-binary PATH] [--server-binary PATH]
#                              [--runtime SECONDS]
#
# Environment:
#   PROSKENION_BINARY     Existing proskenion binary to run.
#   ALETHEIA_BINARY       Existing aletheia server binary to start when no URL is supplied.
#   ALETHEIA_URL          Existing server URL.
#   ALETHEIA_AUTH_TOKEN   Bearer token written to the temporary desktop config.
#   PROSKENION_RUNTIME    Runtime budget in seconds, default: 20.
#   ALETHEIA_SMOKE_KEEP_LOGS  Preserve temporary logs on success when set to 1.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROSKENION_BINARY="${PROSKENION_BINARY:-$REPO_ROOT/crates/theatron/proskenion/target/release/proskenion}"
SERVER_BINARY="${ALETHEIA_BINARY:-$REPO_ROOT/target/debug/aletheia}"
SERVER_URL="${ALETHEIA_URL:-}"
AUTH_TOKEN="${ALETHEIA_AUTH_TOKEN:-}"
RUNTIME="${PROSKENION_RUNTIME:-20}"
KEEP_LOGS="${ALETHEIA_SMOKE_KEEP_LOGS:-0}"
START_SERVER=false
TMPDIR=""
SERVER_PID=""

log() {
    echo "[smoke-proskenion] $*"
}

die() {
    log "ERROR: $*" >&2
    exit 1
}

degrade() {
    log "SKIP: $*" >&2
    exit 2
}

usage() {
    sed -n '4,18p' "$0" | sed 's/^# \{0,1\}//'
}

cleanup() {
    local status=$?
    if [[ -n "${SERVER_PID:-}" ]] && kill -0 "$SERVER_PID" >/dev/null 2>&1; then
        kill "$SERVER_PID" >/dev/null 2>&1 || true # best-effort cleanup
        wait "$SERVER_PID" >/dev/null 2>&1 || true # process may already be gone
    fi
    if [[ -n "${TMPDIR:-}" ]]; then
        if [[ "$status" -ne 0 || "$KEEP_LOGS" == "1" ]]; then
            log "$(printf 'preserving logs under %s' "$LOGDIR")"
        else
            rm -rf "$TMPDIR"
        fi
    fi
}
trap cleanup EXIT

while [[ $# -gt 0 ]]; do
    case "$1" in
        --server-url)
            SERVER_URL="$2"
            shift 2
            ;;
        --auth-token)
            AUTH_TOKEN="$2"
            shift 2
            ;;
        --proskenion-binary)
            PROSKENION_BINARY="$2"
            shift 2
            ;;
        --server-binary)
            SERVER_BINARY="$2"
            shift 2
            ;;
        --runtime)
            RUNTIME="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "unknown argument: $1"
            ;;
    esac
done

[[ "$RUNTIME" =~ ^[0-9]+$ ]] || die "--runtime must be an integer number of seconds"
(( RUNTIME > 0 )) || die "--runtime must be positive"

if [[ ! -x "$PROSKENION_BINARY" ]]; then
    degrade "$(printf 'proskenion binary not found or not executable at %s; build it with scripts/install-proskenion.sh or pass --proskenion-binary' "$PROSKENION_BINARY")"
fi

if [[ -z "$SERVER_URL" ]]; then
    if [[ ! -x "$SERVER_BINARY" ]]; then
        degrade "$(printf 'no --server-url was supplied and aletheia server binary is unavailable at %s; pass --server-url or --server-binary' "$SERVER_BINARY")"
    fi
    START_SERVER=true
fi

if [[ -z "${DISPLAY:-}" ]]; then
    if command -v xvfb-run >/dev/null 2>&1; then
        RUNNER=(xvfb-run -a)
    else
        degrade "DISPLAY is unset and xvfb-run is unavailable; install Xvfb or run from an active desktop session"
    fi
else
    RUNNER=()
fi

TMPDIR="$(mktemp -d)"
LOGDIR="$TMPDIR/logs"
CONFIG_HOME="$TMPDIR/config"
INSTANCE_ROOT="$TMPDIR/instance"
mkdir -p "$LOGDIR" "$CONFIG_HOME/aletheia" "$INSTANCE_ROOT"

if [[ "$START_SERVER" == true ]]; then
    PORT="${ALETHEIA_SMOKE_PORT:-$(( 39000 + (RANDOM % 2000) ))}"
    SERVER_URL="$(printf 'http://127.0.0.1:%s' "$PORT")"
    mkdir -p "$INSTANCE_ROOT/config"
    {
        echo "[gateway]"
        echo 'bind = "localhost"'
        printf 'port = %s\n' "$PORT"
        echo
        echo "[gateway.auth]"
        echo 'mode = "none"'
        echo 'none_role = "operator"'
    } >"$INSTANCE_ROOT/config/aletheia.toml"
    log "$(printf 'starting server: %s --instance-root %s --bind 127.0.0.1 --port %s serve' "$SERVER_BINARY" "$INSTANCE_ROOT" "$PORT")"
    "$SERVER_BINARY" \
        --instance-root "$INSTANCE_ROOT" \
        --bind 127.0.0.1 \
        --port "$PORT" \
        serve >"$LOGDIR/server.log" 2>&1 &
    SERVER_PID=$!

    for _ in $(seq 1 100); do
        if curl -fsS "$SERVER_URL/health" >/dev/null 2>&1; then
            break
        fi
        if ! kill -0 "$SERVER_PID" >/dev/null 2>&1; then
            degrade "$(printf 'started server exited before health check passed; see %s/server.log' "$LOGDIR")"
        fi
        sleep 0.1
    done
fi

if ! curl -fsS "$SERVER_URL/health" >/dev/null 2>&1; then
    degrade "$(printf 'server health check failed for %s; provide a live --server-url or inspect %s/server.log' "$SERVER_URL" "$LOGDIR")"
fi

{
    echo "[connection]"
    printf 'server_url = "%s"\n' "$SERVER_URL"
    echo "auto_reconnect = false"
    echo "connect_timeout_secs = 5"
    if [[ -n "$AUTH_TOKEN" ]]; then
        printf 'auth_token = "%s"\n' "$AUTH_TOKEN"
    fi
} >"$CONFIG_HOME/aletheia/desktop.toml"
chmod 600 "$CONFIG_HOME/aletheia/desktop.toml"

PROSKENION_LOG="$LOGDIR/proskenion.log"
log "$(printf 'running proskenion for %ss against %s' "$RUNTIME" "$SERVER_URL")"
set +e
XDG_CONFIG_HOME="$CONFIG_HOME" \
RUST_LOG="${RUST_LOG:-info}" \
timeout --preserve-status "$RUNTIME" "${RUNNER[@]}" "$PROSKENION_BINARY" \
    >"$PROSKENION_LOG" 2>&1
STATUS=$?
set -e

case "$STATUS" in
    0|143)
        ;;
    124)
        log "runtime budget elapsed; treating bounded timeout as expected smoke completion"
        ;;
    *)
        die "$(printf 'proskenion exited with status %s; see %s' "$STATUS" "$PROSKENION_LOG")"
        ;;
esac

ERROR_PATTERN='cannot open display|Gtk-WARNING|WebKitWebProcess.*ERROR|readPIDFromPeer|failed to connect|connection refused|error sending request|panic|panicked'
if grep -Eiq "$ERROR_PATTERN" "$PROSKENION_LOG"; then
    die "$(printf 'known startup/display/connection error pattern found in %s' "$PROSKENION_LOG")"
fi

if [[ "$KEEP_LOGS" == "1" ]]; then
    log "$(printf 'pass; logs captured under %s' "$LOGDIR")"
else
    log "pass; temporary logs will be removed (set ALETHEIA_SMOKE_KEEP_LOGS=1 to retain them)"
fi
