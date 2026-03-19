#!/usr/bin/env bash
set -euo pipefail
# Automated database backup for aletheia. Designed for cron or systemd timer.
#
# Creates a timestamped JSON export of the session store and prunes old copies.
#
# Usage:
#   scripts/backup-cron.sh [--keep N] [--output-dir DIR]
#
# Environment:
#   ALETHEIA_ROOT       Instance root directory (default: ~/ergon/instance)
#   ALETHEIA_BINARY     Path to the aletheia binary  (default: ~/ergon/bin/aletheia)
#   BACKUP_KEEP         Number of backup files to retain (default: 7)
#   BACKUP_OUTPUT_DIR   Directory for backup files (default: $ALETHEIA_ROOT/backups)
#
# Cron example (daily at 02:00):
#   0 2 * * * /path/to/scripts/backup-cron.sh >> /var/log/aletheia-backup.log 2>&1
#
# Systemd timer: see instance.example/services/ for aletheia-backup.{service,timer}

INSTANCE_ROOT="${ALETHEIA_ROOT:-$HOME/ergon/instance}"
ALETHEIA="${ALETHEIA_BINARY:-$HOME/ergon/bin/aletheia}"
KEEP="${BACKUP_KEEP:-7}"
OUTPUT_DIR="${BACKUP_OUTPUT_DIR:-${INSTANCE_ROOT}/backups}"

# Lock file — prevent concurrent runs
LOCK_FILE="${INSTANCE_ROOT}/backup.lock"

# --- Logging ---

log() {
    echo "[backup] $(date -u +"%Y-%m-%dT%H:%M:%SZ") $*"
}

die() {
    log "ERROR: $*" >&2
    exit 1
}

# --- Argument parsing ---

while [[ $# -gt 0 ]]; do
    case "$1" in
        --keep)
            [[ $# -ge 2 && "$2" =~ ^[0-9]+$ ]] \
                || die "--keep requires a positive integer argument"
            KEEP="$2"
            shift 2
            ;;
        --output-dir)
            [[ $# -ge 2 && -n "$2" ]] \
                || die "--output-dir requires a directory argument"
            OUTPUT_DIR="$2"
            shift 2
            ;;
        *)
            die "Unknown argument: $1"
            ;;
    esac
done

# --- Prerequisite checks ---

[[ -x "$ALETHEIA" ]] \
    || die "aletheia binary not found or not executable: ${ALETHEIA}"

[[ -d "$INSTANCE_ROOT" ]] \
    || die "Instance root not found: ${INSTANCE_ROOT}. Run 'aletheia init' first."

# --- Mutual exclusion ---

exec 9>"${LOCK_FILE}"
flock -n 9 || { log "Another backup is already running (lock: ${LOCK_FILE})"; exit 0; }

# --- Output directory ---

mkdir -p -- "$OUTPUT_DIR"

# --- Run backup ---

timestamp="$(date -u +"%Y%m%dT%H%M%SZ")"
backup_file="${OUTPUT_DIR}/sessions-${timestamp}.json"

log "Starting backup → ${backup_file}"

"$ALETHEIA" -r "$INSTANCE_ROOT" backup --export-json --yes \
    > "$backup_file" \
    || die "aletheia backup command failed"

bytes="$(wc -c < "$backup_file")"
log "Backup complete: ${backup_file} (${bytes} bytes)"

# --- Prune old backups ---

log "Pruning to ${KEEP} most-recent backups in ${OUTPUT_DIR}"

local_count=0
while IFS= read -r old_file; do
    local_count=$(( local_count + 1 ))
    if (( local_count > KEEP )); then
        log "Removing old backup: ${old_file}"
        rm -f -- "$old_file"
    fi
done < <(find "$OUTPUT_DIR" -maxdepth 1 -name 'sessions-*.json' -type f \
    | sort -r)

log "Done. Kept ${KEEP} backups."
