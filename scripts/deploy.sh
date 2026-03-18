#!/usr/bin/env bash
set -euo pipefail
# Deploy aletheia binary to the local instance.
#
# Usage: scripts/deploy.sh [--build] [--restart] [--rollback] [--dry-run]
#   --build      Build release binary before deploying (default: use existing)
#   --restart    Restart systemd service after deploy (default: just copy)
#   --rollback   Restore the most recent backup and restart
#   --dry-run    Show what would happen without executing
#   No flags:    build + copy + restart (full deploy)
#
# Prerequisites: cargo, curl, jq, systemctl

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTANCE_ROOT="${ALETHEIA_ROOT:-$HOME/ergon/instance}"
BINARY_SRC="$REPO_ROOT/target/release/aletheia"
BINARY_DST="${ALETHEIA_BINARY:-$HOME/ergon/bin/aletheia}"
SERVICE="aletheia.service"
BACKUP_DIR="${INSTANCE_ROOT}/.deploy-backup"
DEPLOY_LOG="${INSTANCE_ROOT}/deploy.log"
HEALTH_URL="${ALETHEIA_HEALTH_URL:-http://localhost:18789/api/health}"
HEALTH_TIMEOUT=30
MAX_BACKUPS=3

# --- Logging ---

log() {
    local msg
    msg="[deploy] $(date -u +"%Y-%m-%dT%H:%M:%SZ") $*"
    echo "$msg"
    if [[ "$DRY_RUN" == false ]]; then
        mkdir -p "$(dirname "$DEPLOY_LOG")"
        echo "$msg" >> "$DEPLOY_LOG"
    fi
}

die() {
    log "ERROR: $*" >&2
    exit 1
}

# --- Parse flags ---

BUILD=false
RESTART=false
ROLLBACK=false
DRY_RUN=false

if [[ $# -eq 0 ]]; then
    BUILD=true
    RESTART=true
fi

while [[ $# -gt 0 ]]; do
    case "$1" in
        --build) BUILD=true; shift ;;
        --restart) RESTART=true; shift ;;
        --rollback) ROLLBACK=true; shift ;;
        --dry-run) DRY_RUN=true; shift ;;
        *) die "Unknown flag: $1" ;;
    esac
done

# --- Backup functions ---

backup_binary() {
    if [[ ! -f "$BINARY_DST" ]]; then
        log "No existing binary at $BINARY_DST, skipping backup"
        return 0
    fi

    local timestamp
    timestamp="$(date -u +"%Y%m%dT%H%M%SZ")"
    local backup_path="${BACKUP_DIR}/aletheia.backup.${timestamp}"

    if [[ "$DRY_RUN" == true ]]; then
        log "[dry-run] Would back up $BINARY_DST to $backup_path"
        log "[dry-run] Would prune backups beyond $MAX_BACKUPS"
        return 0
    fi

    mkdir -p "$BACKUP_DIR"
    cp -- "$BINARY_DST" "$backup_path"

    if [[ ! -f "$backup_path" ]]; then
        die "Backup failed: $backup_path not created"
    fi

    log "Backed up binary to $backup_path ($(stat -c%s "$backup_path") bytes)"

    # Prune old backups, keep the newest MAX_BACKUPS
    local -a backups
    mapfile -t backups < <(find "$BACKUP_DIR" -maxdepth 1 -name 'aletheia.backup.*' -type f -printf '%T@\t%p\n' 2>/dev/null \
        | sort -rn | cut -f2-)
    local count=${#backups[@]}
    if (( count > MAX_BACKUPS )); then
        local i
        for (( i = MAX_BACKUPS; i < count; i++ )); do
            log "Pruning old backup: ${backups[$i]}"
            rm -f -- "${backups[$i]}"
        done
    fi
}

get_latest_backup() {
    find "$BACKUP_DIR" -maxdepth 1 -name 'aletheia.backup.*' -type f -printf '%T@\t%p\n' 2>/dev/null \
        | sort -rn | head -1 | cut -f2-
}

# --- Health check ---

check_health() {
    local elapsed=0
    local interval=3

    log "Waiting up to ${HEALTH_TIMEOUT}s for health check..."

    while (( elapsed < HEALTH_TIMEOUT )); do
        if health_response=$(curl -sf --max-time 5 "$HEALTH_URL" 2>/dev/null); then
            local status
            status=$(echo "$health_response" | jq -r '.status // empty' 2>/dev/null) || true  # NOTE: intentional - failure is non-fatal here
            local version
            version=$(echo "$health_response" | jq -r '.version // empty' 2>/dev/null) || true  # NOTE: intentional - failure is non-fatal here

            if [[ "$status" == "healthy" || "$status" == "degraded" ]]; then
                log "Health check passed: $status v${version:-unknown}"
                return 0
            fi
            log "Health status: $status (waiting...)"
        fi

        sleep "$interval"
        elapsed=$(( elapsed + interval ))
    done

    log "Health check failed after ${HEALTH_TIMEOUT}s"
    return 1
}

# --- Smoke test ---

smoke_test() {
    log "Running smoke test (check-config)..."
    if "$BINARY_DST" -r "$INSTANCE_ROOT" check-config; then
        log "Smoke test passed"
    else
        die "Smoke test failed — config is invalid, deploy aborted"
    fi
}

# --- Rollback ---

do_rollback() {
    local backup
    backup="$(get_latest_backup)"

    if [[ -z "$backup" ]]; then
        die "No backups found in $BACKUP_DIR"
    fi

    if [[ "$DRY_RUN" == true ]]; then
        log "[dry-run] Would restore $backup to $BINARY_DST"
        log "[dry-run] Would restart $SERVICE"
        log "[dry-run] Would run health check"
        return 0
    fi

    log "Rolling back from $backup..."

    # Stop service
    if systemctl --user is-active "$SERVICE" &>/dev/null; then
        log "Stopping $SERVICE..."
        systemctl --user stop "$SERVICE"
    fi

    # Restore binary
    cp -- "$backup" "$BINARY_DST"
    log "Restored binary from $backup"

    # Restart service
    systemctl --user daemon-reload
    systemctl --user start "$SERVICE"
    log "Service restarted"

    # Verify health
    if check_health; then
        log "Rollback complete"
    else
        die "Service unhealthy after rollback. Manual intervention required."
    fi
}

# --- Refresh OAuth token ---

refresh_token() {
    local cred_file="$HOME/.claude/.credentials.json"
    if [[ -f "$cred_file" ]]; then
        local token
        token=$(jq -r '.claudeAiOauth.accessToken // empty' "$cred_file" 2>/dev/null) || return 0
        if [[ -n "$token" ]]; then
            local env_file="${INSTANCE_ROOT}/config/env"
            mkdir -p "${INSTANCE_ROOT}/config"
            if [[ -f "$env_file" ]] && grep -q "^ANTHROPIC_AUTH_TOKEN=" "$env_file"; then
                sed -i "s|^ANTHROPIC_AUTH_TOKEN=.*|ANTHROPIC_AUTH_TOKEN=$token|" "$env_file"
            else
                echo "ANTHROPIC_AUTH_TOKEN=$token" >> "$env_file"
            fi
            chmod 600 "$env_file"
            log "Token written to $env_file"
        fi
    fi
}

# --- Main ---

# Handle rollback mode
if [[ "$ROLLBACK" == true ]]; then
    log "=== Rollback requested ==="
    do_rollback
    exit 0
fi

# Prereq: instance directory must exist before any deploy step.
if [[ ! -d "$INSTANCE_ROOT" ]]; then
    die "Instance directory not found: $INSTANCE_ROOT. Run 'aletheia init' first."
fi

log "=== Deploy started ==="

# Build
if [[ "$BUILD" == true ]]; then
    if [[ "$DRY_RUN" == true ]]; then
        log "[dry-run] Would build release binary"
    else
        log "Building release binary..."
        cd "$REPO_ROOT"
        cargo build --release -p aletheia
        log "Built: $(./target/release/aletheia --version)"
    fi
fi

# Verify binary exists
if [[ "$DRY_RUN" == false && ! -f "$BINARY_SRC" ]]; then
    die "Binary not found at $BINARY_SRC. Run with --build or build manually first."
fi

# Backup existing binary before deploy
backup_binary

# Stop service if running
if systemctl --user is-active "$SERVICE" &>/dev/null; then
    if [[ "$DRY_RUN" == true ]]; then
        log "[dry-run] Would stop $SERVICE"
    else
        log "Stopping $SERVICE..."
        systemctl --user stop "$SERVICE"
    fi
fi

# Copy binary
if [[ "$DRY_RUN" == true ]]; then
    log "[dry-run] Would copy $BINARY_SRC to $BINARY_DST"
else
    mkdir -p "$(dirname "$BINARY_DST")"
    cp -- "$BINARY_SRC" "$BINARY_DST"
    log "Deployed: $BINARY_DST"
fi

# Smoke test: validate config with the newly deployed binary
if [[ "$DRY_RUN" == true ]]; then
    log "[dry-run] Would run smoke test (check-config)"
else
    smoke_test
fi

# Refresh token
if [[ "$DRY_RUN" == false ]]; then
    refresh_token
else
    log "[dry-run] Would refresh OAuth token"
fi

# Restart and health check
if [[ "$RESTART" == true ]]; then
    if [[ "$DRY_RUN" == true ]]; then
        log "[dry-run] Would restart $SERVICE"
        log "[dry-run] Would run health check (${HEALTH_TIMEOUT}s timeout)"
        log "[dry-run] Would auto-rollback on health check failure"
    else
        systemctl --user daemon-reload
        systemctl --user start "$SERVICE"
        log "Service started"

        if check_health; then
            log "=== Deploy complete ==="
        else
            log "Health check failed, triggering automatic rollback..."
            do_rollback
            die "Deploy failed. Rolled back to previous version."
        fi
    fi
else
    log "=== Deploy complete (no restart) ==="
fi
