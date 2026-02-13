#!/usr/bin/env bash
#
# Boox Tab Ultra Sync Script
# Bidirectional sync between local philosophy drafts and Boox Pure Writer
#
# Usage: ./boox-sync.sh [push|pull|status]
#

set -euo pipefail

BOOX_HOST="qualcomm-tabultracpro"
BOOX_PORT="8022"
BOOX_USER="u0_a189"
BOOX_WRITING="/sdcard/Documents/Purewriter/the_coherence_of_aporia"
BOOX_POETRY="/sdcard/Documents/Purewriter/poetry"

LOCAL_WRITING="/mnt/ssd/aletheia/demiurge/writing/philosophy"
LOCAL_POETRY="/mnt/ssd/aletheia/demiurge/writing/poetry"
LOG_FILE="/mnt/ssd/aletheia/demiurge/logs/boox-sync.log"

mkdir -p "$(dirname "$LOG_FILE")"

log() {
  echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" | tee -a "$LOG_FILE"
}

check_connection() {
  if ! ssh -o ConnectTimeout=5 -p "$BOOX_PORT" "$BOOX_USER@$BOOX_HOST" 'echo ok' &>/dev/null; then
    log "ERROR: Cannot connect to Boox. Is it awake? Run 'sshd' in Termux."
    exit 1
  fi
}

push() {
  log "=== PUSH: Local → Boox ==="
  check_connection
  
  # Sync chapter folders that have drafts
  for dir in 01-ground 02-aporia-1 03-aporia-2 04-aporia-3 05-aporia-4 06-coherence 07-appendices; do
    if [[ -d "$LOCAL_WRITING/$dir" ]]; then
      # Create dir on Boox if needed
      ssh -p "$BOOX_PORT" "$BOOX_USER@$BOOX_HOST" "mkdir -p $BOOX_WRITING/$dir"
      
      # Sync markdown files
      rsync -avz --update \
        -e "ssh -p $BOOX_PORT" \
        --include="*.md" --exclude="*" \
        "$LOCAL_WRITING/$dir/" \
        "$BOOX_USER@$BOOX_HOST:$BOOX_WRITING/$dir/" 2>/dev/null || true
    fi
  done
  
  log "Push complete"
}

pull() {
  log "=== PULL: Boox → Local ==="
  check_connection
  
  # Sync back from Boox
  for dir in 01-ground 02-aporia-1 03-aporia-2 04-aporia-3 05-aporia-4 06-coherence 07-appendices; do
    if ssh -p "$BOOX_PORT" "$BOOX_USER@$BOOX_HOST" "test -d $BOOX_WRITING/$dir" 2>/dev/null; then
      mkdir -p "$LOCAL_WRITING/$dir"
      
      rsync -avz --update \
        -e "ssh -p $BOOX_PORT" \
        --include="*.md" --exclude="*" \
        "$BOOX_USER@$BOOX_HOST:$BOOX_WRITING/$dir/" \
        "$LOCAL_WRITING/$dir/" 2>/dev/null || true
    fi
  done
  
  # Also pull any new files from Purewriter root
  rsync -avz --update \
    -e "ssh -p $BOOX_PORT" \
    --include="*.md" --exclude="*/" --exclude="*" \
    "$BOOX_USER@$BOOX_HOST:$BOOX_WRITING/" \
    "$LOCAL_WRITING/drafts/boox/" 2>/dev/null || true
  
  log "Pull complete"
}

sync() {
  log "=== BIDIRECTIONAL SYNC ==="
  pull  # Get changes from Boox first
  push  # Then push any local updates
  log "Sync complete"
}

status() {
  echo "=== BOOX CONNECTION STATUS ==="
  if ssh -o ConnectTimeout=5 -p "$BOOX_PORT" "$BOOX_USER@$BOOX_HOST" 'echo "Connected"' 2>/dev/null; then
    echo "Host: $BOOX_HOST"
    echo "Port: $BOOX_PORT"
    echo "User: $BOOX_USER"
    echo ""
    echo "=== BOOX WRITING FILES ==="
    ssh -p "$BOOX_PORT" "$BOOX_USER@$BOOX_HOST" "find $BOOX_WRITING -name '*.md' -type f 2>/dev/null"
  else
    echo "Cannot connect to Boox. Make sure:"
    echo "  1. Boox screen is on"
    echo "  2. Termux is open"
    echo "  3. Run 'sshd' in Termux"
  fi
}

case "${1:-status}" in
  push) push ;;
  pull) pull ;;
  sync) sync ;;
  status) status ;;
  *)
    echo "Usage: $0 [push|pull|sync|status]"
    echo ""
    echo "  push   - Local → Boox"
    echo "  pull   - Boox → Local"
    echo "  sync   - Bidirectional (pull then push)"
    echo "  status - Check connection and list files"
    exit 1
    ;;
esac
