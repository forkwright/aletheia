#!/usr/bin/env bash
# rollback.sh — Restore previous Aletheia deployment
#
# Usage: ./scripts/rollback.sh [backup-timestamp]

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BACKUP_DIR="$REPO_ROOT/.deploy-backup"

log() { echo "[rollback] $(date +%H:%M:%S) $*"; }
die() { echo "[rollback] ERROR: $*" >&2; exit 1; }

# Find the latest backup or use specified timestamp
if [[ -n "${1:-}" ]]; then
  BACKUP="$BACKUP_DIR/$1"
else
  BACKUP=$(find "$BACKUP_DIR" -maxdepth 1 -mindepth 1 -type d -printf '%T@\t%p\n' | sort -rn | head -1 | cut -f2-)
fi

[[ -d "$BACKUP" ]] || die "No backup found at $BACKUP"

log "Rolling back from $BACKUP..."

# Restore binary
if [[ -f "$BACKUP/aletheia" ]]; then
  cp "$BACKUP/aletheia" "$REPO_ROOT/target/release/aletheia"
  log "Binary restored"
fi

# Show the git SHA for reference
if [[ -f "$BACKUP/git-sha" ]]; then
  log "Backup was from commit: $(cat "$BACKUP/git-sha")"
fi

# Restart
log "Restarting daemon..."
sudo systemctl restart aletheia || die "Restart failed after rollback"

sleep 3
if systemctl is-active --quiet aletheia; then
  log "✓ Rollback complete. Daemon is running."
else
  die "Daemon failed to start after rollback"
fi
