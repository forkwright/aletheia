#!/usr/bin/env bash
set -euo pipefail

# User-systemd entrypoint for the D3 prosoche heartbeat.
#
# Keep these defaults synchronized with:
#   - instance.example/services/aletheia-health.timer (OnUnitActiveSec)
#   - docs/CONFIGURATION.md [maintenance.prosoche.externalTimer]
# Do not parse aletheia.toml here; env vars are the contract with the timer unit.

ALETHEIA_BIN="${ALETHEIA_BIN:-aletheia}"
ALETHEIA_URL="${ALETHEIA_URL:-http://127.0.0.1:18789}"
HEARTBEAT_TASK="${ALETHEIA_HEARTBEAT_TASK:-prosoche-self-audit}"
HEARTBEAT_INTERVAL_SECS="${ALETHEIA_HEARTBEAT_INTERVAL_SECS:-300}"

timestamp() {
    date -u +"%Y-%m-%dT%H:%M:%SZ"
}

log() {
    printf '%s %s\n' "$(timestamp)" "$*"
}

log "heartbeat: task=${HEARTBEAT_TASK} interval=${HEARTBEAT_INTERVAL_SECS}s url=${ALETHEIA_URL}"
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
