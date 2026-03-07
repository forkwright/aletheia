#!/usr/bin/env bash
# deploy.sh — Build and deploy Aletheia
#
# Usage: ./scripts/deploy.sh [--skip-ui] [--dry-run]
#
# Steps:
#   1. Pull latest main
#   2. Build Rust binary (release)
#   3. Build UI (optional, vite)
#   4. Validate config (aletheia health)
#   5. Restart daemon
#
# Rollback: ./scripts/rollback.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
UI_DIR="$REPO_ROOT/ui"
BACKUP_DIR="$REPO_ROOT/.deploy-backup"
TIMESTAMP=$(date +%Y%m%d-%H%M%S)

SKIP_UI=false
DRY_RUN=false

for arg in "$@"; do
  case $arg in
    --skip-ui) SKIP_UI=true ;;
    --dry-run) DRY_RUN=true ;;
    *) echo "Unknown option: $arg"; exit 1 ;;
  esac
done

log() { echo "[deploy] $(date +%H:%M:%S) $*"; }
die() { echo "[deploy] ERROR: $*" >&2; exit 1; }

# 1. Pull latest
log "Pulling latest main..."
cd "$REPO_ROOT"
git checkout main
git pull --rebase origin main

# 2. Build Rust binary
log "Building release binary..."
cargo build --release || die "Rust build failed"

# 3. Build UI (optional)
if [[ "$SKIP_UI" == "false" ]] && [[ -d "$UI_DIR" ]]; then
  log "Building UI..."
  cd "$UI_DIR"
  npm ci || die "UI dependency install failed"
  npm run build || die "UI build failed"
else
  log "Skipping UI build"
fi

# 4. Back up current binary
log "Backing up current binary to $BACKUP_DIR/$TIMESTAMP..."
mkdir -p "$BACKUP_DIR/$TIMESTAMP"
if [[ -f "$REPO_ROOT/target/release/aletheia" ]]; then
  cp "$REPO_ROOT/target/release/aletheia" "$BACKUP_DIR/$TIMESTAMP/aletheia"
fi
git rev-parse HEAD > "$BACKUP_DIR/$TIMESTAMP/git-sha"

# Keep only last 5 backups
mapfile -t old_backups < <(find "$BACKUP_DIR" -maxdepth 1 -mindepth 1 -type d -printf '%T@\t%p\n' | sort -rn | tail -n +6 | cut -f2-)
for f in "${old_backups[@]}"; do rm -rf "$f"; done

if [[ "$DRY_RUN" == "true" ]]; then
  log "Dry run — skipping restart. Binary built and backed up."
  exit 0
fi

# 5. Validate
log "Validating..."
"$REPO_ROOT/target/release/aletheia" health || log "Warning: health check failed (may not be running yet)"

# 6. Restart daemon
log "Restarting aletheia daemon..."
sudo systemctl restart aletheia || die "Daemon restart failed"

# 7. Verify
sleep 3
if systemctl is-active --quiet aletheia; then
  log "✓ Deploy complete. Daemon is running."
  log "  Backup: $BACKUP_DIR/$TIMESTAMP"
  log "  Rollback: ./scripts/rollback.sh"
else
  log "⚠ Daemon failed to start. Rolling back..."
  "$REPO_ROOT/scripts/rollback.sh"
  die "Deploy failed — rolled back to previous version"
fi
