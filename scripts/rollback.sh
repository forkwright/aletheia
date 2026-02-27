#!/usr/bin/env bash
# rollback.sh — Restore previous Aletheia deployment
#
# Usage: ./scripts/rollback.sh [backup-timestamp]
#
# Without arguments, restores the most recent backup.
# With a timestamp argument (e.g. 20260227-143052), restores that specific backup.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUNTIME_DIR="$REPO_ROOT/infrastructure/runtime"
UI_DIR="$REPO_ROOT/ui"
BACKUP_DIR="$REPO_ROOT/.deploy-backup"

log() { echo "[rollback] $(date +%H:%M:%S) $*"; }
die() { echo "[rollback] ERROR: $*" >&2; exit 1; }

# Find backup to restore
if [[ $# -gt 0 ]]; then
  TARGET="$BACKUP_DIR/$1"
else
  TARGET=$(ls -dt "$BACKUP_DIR"/*/ 2>/dev/null | head -1)
fi

[[ -z "${TARGET:-}" || ! -d "$TARGET" ]] && die "No backup found at ${TARGET:-$BACKUP_DIR/}"

log "Restoring from: $TARGET"

# Show what we're rolling back to
if [[ -f "$TARGET/git-sha" ]]; then
  log "  Git SHA: $(cat "$TARGET/git-sha")"
fi

# Restore runtime
if [[ -d "$TARGET/runtime-dist" ]]; then
  log "Restoring runtime artifacts..."
  rm -rf "$RUNTIME_DIR/dist"
  cp -r "$TARGET/runtime-dist" "$RUNTIME_DIR/dist"
else
  log "No runtime backup — skipping"
fi

# Restore UI
if [[ -d "$TARGET/ui-dist" ]]; then
  log "Restoring UI artifacts..."
  rm -rf "$UI_DIR/dist"
  cp -r "$TARGET/ui-dist" "$UI_DIR/dist"
else
  log "No UI backup — skipping"
fi

# Restart daemon
log "Restarting daemon..."
sudo systemctl restart aletheia || die "Daemon restart failed after rollback"

sleep 3
if systemctl is-active --quiet aletheia; then
  log "✓ Rollback complete. Daemon is running."
else
  die "Daemon failed to start even after rollback. Manual intervention required."
fi
