#!/usr/bin/env bash
set -euo pipefail

# User-systemd entrypoint for the D3 prosoche heartbeat.

ALETHEIA_BIN="${ALETHEIA_BIN:-aletheia}"
ALETHEIA_URL="${ALETHEIA_URL:-http://127.0.0.1:18789}"
HEARTBEAT_TASK="${ALETHEIA_HEARTBEAT_TASK:-prosoche-self-audit}"

timestamp() {
    date -u +"%Y-%m-%dT%H:%M:%SZ"
}

log() {
    printf '%s %s\n' "$(timestamp)" "$*"
}

log "heartbeat: checking server at ${ALETHEIA_URL}"
if ! "${ALETHEIA_BIN}" health --url "${ALETHEIA_URL}"; then
    log "heartbeat: server unavailable or unhealthy"
    exit 1
fi

log "heartbeat: running maintenance task ${HEARTBEAT_TASK}"
if ! "${ALETHEIA_BIN}" maintenance run "${HEARTBEAT_TASK}"; then
    log "heartbeat: maintenance task failed: ${HEARTBEAT_TASK}"
    exit 1
fi

log "heartbeat: complete"
