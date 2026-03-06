#!/usr/bin/env bash
# deploy.sh — Build and deploy Aletheia runtime + UI
#
# Usage: ./scripts/deploy.sh [--skip-ui] [--skip-runtime] [--dry-run]
#
# Steps:
#   1. Pull latest main
#   2. Build runtime (tsdown)
#   3. Build UI (vite)
#   4. Back up current artifacts
#   5. Copy new artifacts into place
#   6. Validate config (aletheia doctor)
#   7. Restart daemon (systemctl restart aletheia)
#
# Rollback: ./scripts/rollback.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUNTIME_DIR="$REPO_ROOT/infrastructure/runtime"
UI_DIR="$REPO_ROOT/ui"
BACKUP_DIR="$REPO_ROOT/.deploy-backup"
TIMESTAMP=$(date +%Y%m%d-%H%M%S)

SKIP_UI=false
SKIP_RUNTIME=false
DRY_RUN=false

for arg in "$@"; do
  case $arg in
    --skip-ui) SKIP_UI=true ;;
    --skip-runtime) SKIP_RUNTIME=true ;;
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

# 2. Build runtime
if [[ "$SKIP_RUNTIME" == "false" ]]; then
  log "Building runtime..."
  cd "$RUNTIME_DIR"
  npm ci || die "Runtime dependency install failed"
  npm run build || die "Runtime build failed"
else
  log "Skipping runtime build"
fi

# 3. Build UI
if [[ "$SKIP_UI" == "false" ]]; then
  log "Building UI..."
  cd "$UI_DIR"
  npm ci || die "UI dependency install failed"
  npm run build || die "UI build failed"
else
  log "Skipping UI build"
fi

# 4. Back up current artifacts
log "Backing up current artifacts to $BACKUP_DIR/$TIMESTAMP..."
mkdir -p "$BACKUP_DIR/$TIMESTAMP"
if [[ -f "$RUNTIME_DIR/dist/entry.mjs" ]]; then
  cp -r "$RUNTIME_DIR/dist" "$BACKUP_DIR/$TIMESTAMP/runtime-dist"
fi
if [[ -d "$UI_DIR/dist" ]]; then
  cp -r "$UI_DIR/dist" "$BACKUP_DIR/$TIMESTAMP/ui-dist"
fi
# Record git SHA for rollback reference
git rev-parse HEAD > "$BACKUP_DIR/$TIMESTAMP/git-sha"

# Keep only last 5 backups
mapfile -t old_backups < <(find "$BACKUP_DIR" -maxdepth 1 -mindepth 1 -type d -printf '%T@\t%p\n' | sort -rn | tail -n +6 | cut -f2-)
for f in "${old_backups[@]}"; do rm -rf "$f"; done

if [[ "$DRY_RUN" == "true" ]]; then
  log "Dry run — skipping restart. Artifacts built and backed up."
  exit 0
fi

# 5. Validate config
log "Validating config..."
if command -v aletheia &>/dev/null; then
  aletheia doctor || die "Config validation failed — NOT deploying"
else
  log "aletheia CLI not in PATH — skipping doctor check"
fi

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
