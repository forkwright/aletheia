#!/usr/bin/env bash
set -euo pipefail
# Restore previous Aletheia deployment from backup.
#
# Usage: scripts/rollback.sh [backup-file]
#   backup-file  Specific backup file path (default: most recent)
#
# Prefer `scripts/deploy.sh --rollback` for integrated rollback with
# logging and health verification.

INSTANCE_ROOT="${ALETHEIA_ROOT:-$HOME/aletheia/instance}"
BINARY_DST="${ALETHEIA_BINARY:-$HOME/.local/bin/aletheia}"
BACKUP_DIR="${INSTANCE_ROOT}/.deploy-backup"
SERVICE="aletheia.service"
HEALTH_URL="${ALETHEIA_HEALTH_URL:-http://localhost:18789/api/health}"

log() { echo "[rollback] $(date -u +"%Y-%m-%dT%H:%M:%SZ") $*"; }
die() { echo "[rollback] $(date -u +"%Y-%m-%dT%H:%M:%SZ") ERROR: $*" >&2; exit 1; }

# Find backup
if [[ -n "${1:-}" ]]; then
    BACKUP="$1"
else
    BACKUP=$(find "$BACKUP_DIR" -maxdepth 1 -name 'aletheia.backup.*' -type f -printf '%T@\t%p\n' 2>/dev/null \
        | sort -rn | head -1 | cut -f2-)
fi

[[ -f "$BACKUP" ]] || die "No backup found at ${BACKUP:-$BACKUP_DIR/}"

log "Rolling back from ${BACKUP}..."

# Stop service
if systemctl --user is-active "$SERVICE" &>/dev/null; then
    log "Stopping ${SERVICE}..."
    systemctl --user stop "$SERVICE"
fi

# Restore binary
cp -- "$BACKUP" "$BINARY_DST"
log "Binary restored to ${BINARY_DST}"

# Restart
log "Starting ${SERVICE}..."
systemctl --user daemon-reload
systemctl --user start "$SERVICE"

check_health() {
    local elapsed=0
    while (( elapsed < 15 )); do
        if curl -sf --max-time 5 "$HEALTH_URL" &>/dev/null; then
            return 0
        fi
        sleep 3
        elapsed=$(( elapsed + 3 ))
    done
    return 1
}

# Health check (15s timeout for rollback)
if check_health; then
    log "Rollback complete. Service is healthy."
elif systemctl --user is-active "$SERVICE" &>/dev/null; then
    log "Rollback complete. Service is running (health endpoint not yet responding)."
else
    die "Service failed to start after rollback"
fi
