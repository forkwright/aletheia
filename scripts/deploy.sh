#!/usr/bin/env bash
set -euo pipefail
# Deploy aletheia binary to the local instance.
#
# Usage: scripts/deploy.sh [--build] [--restart] [--rollback] [--dry-run]
#   --build      Build release binary before deploying (default: use existing)
#   --restart    Restart systemd service after deploy (default: just copy)
#   --rollback   Restore the most recent backup and restart
#   --dry-run    Show what would happen without executing
#   --download vX.Y.Z  Download a verified prebuilt release (fails closed)
#   No flags:    build + copy + restart (full deploy)
#
# Path discovery (first match wins):
#   Instance root:  "$ALETHEIA_ROOT" > ~/aletheia/instance
#   Binary dest:    "$ALETHEIA_BIN" > "$ALETHEIA_BINARY" > ~/.local/bin/aletheia
#
# Prerequisites: cargo, curl, jq, systemctl

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CARGO_TARGET_DIR="$REPO_ROOT/target"

# Instance root: env var, then common locations, then fail
if [[ -n "${ALETHEIA_ROOT:-}" ]]; then
    INSTANCE_ROOT="$ALETHEIA_ROOT"
elif [ -d "$HOME/aletheia/instance" ]; then
    INSTANCE_ROOT="$HOME/aletheia/instance"
else
    echo "[deploy] ERROR: No instance root found. Set ALETHEIA_ROOT or create ~/aletheia/instance/" >&2
    exit 1
fi

BINARY_SRC="$REPO_ROOT/target/release/aletheia"

# Binary destination: env var, then common locations
if [[ -n "${ALETHEIA_BIN:-}" ]]; then
    BINARY_DST="$ALETHEIA_BIN"
elif [[ -n "${ALETHEIA_BINARY:-}" ]]; then
    BINARY_DST="$ALETHEIA_BINARY"
else
    BINARY_DST="$HOME/.local/bin/aletheia"
fi
SERVICE="aletheia.service"
BACKUP_DIR="${INSTANCE_ROOT}/.deploy-backup"
DEPLOY_LOG="${INSTANCE_ROOT}/deploy.log"
HEALTH_URL="${ALETHEIA_HEALTH_URL:-http://localhost:18789/api/health}"
HEALTH_TIMEOUT=30
MAX_BACKUPS=3
DEPLOY_BACKUP=""

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

log_warn() {
    log "WARNING: $*" >&2
}

log_error() {
    log "ERROR: $*" >&2
}

die() {
    log_error "$*"
    exit 1
}

# --- Parse flags ---

BUILD=false
RESTART=false
ROLLBACK=false
DRY_RUN=false
DOWNLOAD_VERSION=""

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
        --download)
            if [[ $# -lt 2 || "$2" == --* ]]; then
                die "Missing version argument for --download (e.g. --download v0.12.0)"
            fi
            DOWNLOAD_VERSION="$2"
            shift 2
            ;;
        *) die "Unknown flag: $1" ;;
    esac
done

# --- Backup functions ---

backup_binary() {
    if [[ ! -f "$BINARY_DST" ]]; then
        log "No existing binary at ${BINARY_DST}, skipping backup"
        return 0
    fi

    local timestamp
    timestamp="$(date -u +"%Y%m%dT%H%M%SZ")"
    local backup_path="${BACKUP_DIR}/aletheia.backup.${timestamp}"

    if [[ "$DRY_RUN" == true ]]; then
        log "[dry-run] Would back up ${BINARY_DST} to $backup_path"
        log "[dry-run] Would prune backups beyond ${MAX_BACKUPS}"
        return 0
    fi

    mkdir -p "$BACKUP_DIR"
    cp -- "$BINARY_DST" "$backup_path"

    if [[ ! -f "$backup_path" ]]; then
        die "Backup failed: $backup_path not created"
    fi
    DEPLOY_BACKUP="$backup_path"

    log "Backed up binary to $backup_path ($(wc -c < "$backup_path" | tr -d ' ') bytes)"

    # Prune old backups, keep the newest MAX_BACKUPS
    local prune_target
    while IFS= read -r prune_target; do
        [[ -n "$prune_target" ]] || continue
        log "Pruning old backup: ${prune_target}"
        rm -f -- "$prune_target"
    done < <(find "$BACKUP_DIR" -maxdepth 1 -name 'aletheia.backup.*' -type f -exec ls -1t {} + 2>/dev/null \
        | tail -n +$((MAX_BACKUPS + 1)))
}

get_latest_backup() {
    find "$BACKUP_DIR" -maxdepth 1 -name 'aletheia.backup.*' -type f -exec ls -1t {} + 2>/dev/null \
        | head -1
}

# --- Liveness check ---

check_liveness() {
    local elapsed=0
    local interval=3

    # `/api/health` is deliberately a public liveness route. It proves that
    # the process answers; it does not carry version or subsystem readiness.
    log "Waiting up to ${HEALTH_TIMEOUT}s for liveness..."

    while (( elapsed < HEALTH_TIMEOUT )); do
        if health_response=$(curl -sf --max-time 5 "$HEALTH_URL" 2>/dev/null); then
            local status
            if ! status=$(echo "$health_response" | jq -r '.status // empty' 2>&1); then
                log_warn "jq failed parsing health status: ${status}"
                status=""
            fi
            if [[ "$status" == "healthy" ]]; then
                log "Liveness check passed: $status"
                return 0
            fi
            log "Liveness status: $status (waiting...)"
        fi

        sleep "$interval"
        elapsed=$(( elapsed + interval ))
    done

    log "Liveness check failed after ${HEALTH_TIMEOUT}s"
    return 1
}

# --- Smoke test ---

smoke_test() {
    local binary_path="${1:-$BINARY_DST}"
    log "Running smoke test (check-config) against ${binary_path}..."
    local smoke_output
    if smoke_output=$("$binary_path" -r "$INSTANCE_ROOT" check-config 2>&1); then
        log "Smoke test passed"
        return 0
    else
        log_error "Smoke test output: ${smoke_output}"
        log_error "Smoke test failed — config is invalid"
        return 1
    fi
}

probe_service_state() {
    local observed=""
    observed=$(systemctl --user is-active "$SERVICE" 2>/dev/null) || true
    case "$observed" in
        active|activating|reloading|deactivating|maintenance|refreshing)
            printf '%s\n' active
            ;;
        inactive|failed)
            printf '%s\n' inactive
            ;;
        *)
            log_error "Could not classify ${SERVICE} state (observed: ${observed:-no output})"
            return 1
            ;;
    esac
}

# --- Rollback ---

do_rollback() {
    local backup="${1:-}"
    if [[ -z "$backup" ]]; then
        backup="$(get_latest_backup)"
    fi

    if [[ -z "$backup" ]]; then
        die "No backups found in ${BACKUP_DIR}"
    fi

    if [[ "$DRY_RUN" == true ]]; then
        log "[dry-run] Would restore $backup to ${BINARY_DST}"
        log "[dry-run] Would restart ${SERVICE}"
        log "[dry-run] Would run liveness check"
        return 0
    fi

    log "Rolling back from $backup..."

    # Stage the rollback executable before touching the running service. A
    # broken backup or full filesystem must leave the current process alone.
    local rollback_tmp
    rollback_tmp=$(mktemp "$(dirname "$BINARY_DST")/aletheia.rollback.XXXXXXXXXX") \
        || die "Failed to create temp file for rollback (mktemp failed)"
    if ! install -m 0755 -- "$backup" "$rollback_tmp"; then
        rm -f -- "$rollback_tmp"
        die "Failed to stage rollback binary"
    fi
    if ! smoke_test "$rollback_tmp"; then
        rm -f -- "$rollback_tmp"
        die "Rollback backup failed validation; live binary and service unchanged"
    fi

    local rollback_service_was_active=false
    local rollback_service_state
    rollback_service_state=$(probe_service_state) \
        || { rm -f -- "$rollback_tmp"; die "Failed to determine service state for rollback"; }
    if [[ "$rollback_service_state" == active ]]; then
        rollback_service_was_active=true
        log "Stopping ${SERVICE}..."
        if ! systemctl --user stop "$SERVICE"; then
            rm -f -- "$rollback_tmp"
            systemctl --user start "$SERVICE" \
                || log_error "Failed to restart unchanged ${SERVICE} after rollback stop error"
            die "Failed to stop ${SERVICE} for rollback"
        fi
    fi

    # Restore binary (atomic: write to temp on same filesystem, then rename)
    if ! mv -- "$rollback_tmp" "$BINARY_DST"; then
        rm -f -- "$rollback_tmp"
        if [[ "$rollback_service_was_active" == true ]]; then
            systemctl --user start "$SERVICE" \
                || log_error "Failed to restart unchanged ${SERVICE} after rollback install failure"
        fi
        die "Failed to install rollback binary"
    fi
    log "Restored binary from $backup"

    # Restart the restored executable even if daemon-reload itself fails. The
    # previously loaded unit remains the best recovery path; dying before the
    # start attempt would strand a service we just stopped.
    local rollback_reload_ok=true
    if ! systemctl --user daemon-reload; then
        rollback_reload_ok=false
        log_error "Failed to reload ${SERVICE} after rollback; attempting the loaded unit"
    fi
    systemctl --user start "$SERVICE" \
        || die "Failed to start restored ${SERVICE} after rollback"
    log "Service restarted"

    # The public route is a liveness-only recovery witness.
    if check_liveness; then
        if [[ "$rollback_reload_ok" == true ]]; then
            log "Rollback complete"
        else
            die "Rollback restored a live service, but daemon-reload failed"
        fi
    else
        die "Service is not live after rollback. Manual intervention required."
    fi
}

abort_fresh_deploy() {
    local reason="$1"
    # A failed start may still leave a process behind, and a failed liveness
    # check necessarily does. Stop it before unlinking its executable.
    if ! systemctl --user stop "$SERVICE"; then
        die "${reason}; failed to stop ${SERVICE}, so candidate was retained"
    fi
    if ! rm -f -- "$BINARY_DST"; then
        die "${reason}; service stopped but candidate removal failed"
    fi
    die "${reason}; candidate removed (no prior binary to restore)"
}

# --- Download prebuilt binary ---

download_binary() {
    local version="$1"
    local repo="${GITHUB_REPO:-$(git -C "$REPO_ROOT" remote get-url origin 2>/dev/null | sed 's|.*github.com[:/]||;s|\.git$||')}"
    repo="${repo:-forkwright/aletheia}"
    # WHY: asset names match the release workflow matrix artifacts, not uname output.
    # Linux: aletheia-linux-x86_64; macOS Apple Silicon: aletheia-macos-aarch64.
    local asset_name
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64)   asset_name="aletheia-linux-x86_64" ;;
        Darwin-arm64)   asset_name="aletheia-macos-aarch64" ;;
        *)
            log "ERROR: no prebuilt binary for $(uname -s)-$(uname -m)"
            return 1
            ;;
    esac
    local tmp_bin tmp_checksum
    tmp_bin="$(mktemp)" || return 1
    tmp_checksum="$(mktemp)" || { rm -f -- "$tmp_bin"; return 1; }
    trap 'rm -f -- "$tmp_bin" "$tmp_checksum"' RETURN

    log "Attempting to download ${version} from ${repo}..."

    # WHY: release workflow uploads binary as ${artifact}-${version} (e.g. aletheia-linux-x86_64-0.31.1).
    # Strip the leading 'v' from the version tag for the asset filename.
    local ver_bare="${version#v}"
    local versioned_asset="${asset_name}-${ver_bare}"
    local checksum_asset="${versioned_asset}.sha256"

    install_verified_download() {
        if ! "$REPO_ROOT/scripts/verify-sha256.sh" \
            "$tmp_bin" "$tmp_checksum" "$versioned_asset"; then
            log "WARNING: checksum verification failed for ${versioned_asset}"
            return 1
        fi
        chmod +x -- "$tmp_bin" \
            || { log "WARNING: could not mark verified download executable"; return 1; }
        local install_tmp
        # A clean checkout has no target/release directory yet.  The verified
        # download is the thing that creates that build-equivalent surface;
        # falling back to Cargo here would defeat --download.
        mkdir -p -- "$(dirname "$BINARY_SRC")" \
            || { log "WARNING: could not create verified-download staging directory"; return 1; }
        install_tmp=$(mktemp "$(dirname "$BINARY_SRC")/aletheia.dl.XXXXXXXXXX") \
            || { log "WARNING: mktemp failed for verified download"; return 1; }
        if ! install -m 0755 -- "$tmp_bin" "$install_tmp"; then
            rm -f -- "$install_tmp"
            log "WARNING: could not stage verified download"
            return 1
        fi
        if ! mv -- "$install_tmp" "$BINARY_SRC"; then
            rm -f -- "$install_tmp"
            log "WARNING: could not install verified download"
            return 1
        fi
        return 0
    }

    # Try authenticated gh first, but never install from a draft. The public
    # curl URL cannot expose drafts and remains the safe unauthenticated path.
    if command -v gh &>/dev/null; then
        local draft_state=""
        if draft_state=$(GH_HTTP_TIMEOUT=120 gh release view "$version" \
            --repo "$repo" --json isDraft --jq .isDraft 2>/dev/null); then
            if [[ "$draft_state" == "true" ]]; then
                log "ERROR: ${version} is still a draft; refusing an unverified release"
                return 1
            fi
            if [[ "$draft_state" != "false" ]]; then
                log "ERROR: ${version} returned an invalid draft state"
                return 1
            fi
        else
            log "WARNING: gh could not prove ${version} is public, trying the public URL..."
            draft_state="unknown"
        fi

        if [[ "$draft_state" == "false" ]] \
            && GH_HTTP_TIMEOUT=120 gh release download "$version" \
            --repo "$repo" \
            --pattern "$versioned_asset" \
            --output "$tmp_bin" \
            --clobber 2>/dev/null \
            && GH_HTTP_TIMEOUT=120 gh release download "$version" \
                --repo "$repo" \
                --pattern "$checksum_asset" \
                --output "$tmp_checksum" \
                --clobber 2>/dev/null; then
            if install_verified_download; then
                log "Downloaded and verified binary via gh: ${BINARY_SRC}"
                return 0
            fi
        fi
        log "WARNING: exact gh release download failed, trying curl..."
    fi

    local url="https://github.com/${repo}/releases/download/${version}/${versioned_asset}"
    local checksum_url="${url}.sha256"
    if curl -fsSL --max-time 120 --output "$tmp_bin" -- "$url" 2>/dev/null \
        && curl -fsSL --max-time 120 --output "$tmp_checksum" -- "$checksum_url" 2>/dev/null; then
        if install_verified_download; then
            log "Downloaded and verified binary via curl: ${BINARY_SRC}"
            return 0
        fi
    fi

    log "ERROR: no verified ${version} release artifact could be installed"
    return 1
}

# --- Main ---

# Handle rollback mode
if [[ "$ROLLBACK" == true ]]; then
    log "=== Rollback requested ==="
    do_rollback
    exit 0
fi

# Download binary if requested
if [[ -n "$DOWNLOAD_VERSION" ]]; then
    [[ "$DOWNLOAD_VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]] \
        || die "Version must match vX.Y.Z format, got: ${DOWNLOAD_VERSION}"
    if [[ "$DRY_RUN" == true ]]; then
        log "[dry-run] Would download ${DOWNLOAD_VERSION} from GitHub releases"
    elif download_binary "$DOWNLOAD_VERSION"; then
        BUILD=false
    else
        die "Download requested for ${DOWNLOAD_VERSION}; refusing an unrequested source build"
    fi
fi

# Prereq: instance directory must exist before any deploy step.
if [[ ! -d "$INSTANCE_ROOT" ]]; then
    die "Instance directory not found: ${INSTANCE_ROOT}. Run 'aletheia init' first."
fi

log "=== Deploy started ==="

# Build
if [[ "$BUILD" == true ]]; then
    if [[ "$DRY_RUN" == true ]]; then
        log "[dry-run] Would build release binary"
    else
        log "Building release binary..."
        cd "$REPO_ROOT"
        CARGO_TARGET_DIR="$REPO_ROOT/target" cargo build --release -p aletheia
        log "Built: $(./target/release/aletheia --version)"
    fi
fi

# Verify binary exists
if [[ "$DRY_RUN" == false && ! -f "$BINARY_SRC" ]]; then
    die "Binary not found at ${BINARY_SRC}. Run with --build or build manually first."
fi

# Copy the candidate to a same-filesystem temp and validate it before any
# service transition. A bad candidate must leave both the live binary and the
# current service state untouched.
BINARY_TMP=""
if [[ "$DRY_RUN" == true ]]; then
    log "[dry-run] Would copy ${BINARY_SRC} to temp location, validate, then atomic move to ${BINARY_DST}"
    log "[dry-run] Would run smoke test (check-config) against temp binary"
    log "[dry-run] Would preserve previous binary at ${BINARY_DST}.prev"
else
    mkdir -p "$(dirname "$BINARY_DST")"

    # Create temp file in same directory as destination for atomic mv
    BINARY_TMP=$(mktemp "$(dirname "$BINARY_DST")/aletheia.XXXXXXXXXX") \
        || die "Failed to create temp file for binary validation (mktemp failed)"

    # Copy to the 0600 mktemp path with the shipped executable mode restored;
    # otherwise the validation invocation below fails before it can inspect
    # the downloaded or freshly built binary.
    if ! install -m 0755 -- "$BINARY_SRC" "$BINARY_TMP"; then
        rm -f -- "$BINARY_TMP"
        die "Failed to stage candidate binary"
    fi

    # Smoke test: validate config with the temp binary
    if ! smoke_test "$BINARY_TMP"; then
        rm -f -- "$BINARY_TMP"
        die "Smoke test failed — production binary unchanged"
    fi
fi

service_was_active=false
service_was_stopped=false
if [[ "$RESTART" == true ]]; then
    service_state=$(probe_service_state) \
        || { rm -f -- "$BINARY_TMP"; die "Failed to determine service state; production unchanged"; }
    if [[ "$service_state" == active ]]; then
        service_was_active=true
    fi
fi

# Back up only after the candidate proves it can read the current config and,
# for restart deployments, the service state is unambiguous.
backup_binary

if [[ "$DRY_RUN" == true ]]; then
    if [[ "$RESTART" == true && "$service_was_active" == true ]]; then
        log "[dry-run] Would stop ${SERVICE} after candidate validation"
    fi
else
    if [[ -f "$BINARY_DST" ]]; then
        if ! cp -- "$BINARY_DST" "${BINARY_DST}.prev"; then
            rm -f -- "$BINARY_TMP"
            die "Failed to preserve previous binary"
        fi
        log "Previous binary preserved at ${BINARY_DST}.prev"
    fi

    # `--download` without `--restart` is an atomic file replacement only.
    # Never stop an active service that this invocation does not own restarting.
    if [[ "$RESTART" == true && "$service_was_active" == true ]]; then
        log "Stopping ${SERVICE}..."
        if ! systemctl --user stop "$SERVICE"; then
            rm -f -- "$BINARY_TMP"
            systemctl --user start "$SERVICE" \
                || log_error "Failed to restart unchanged ${SERVICE} after stop error"
            die "Failed to stop ${SERVICE}; production binary unchanged"
        fi
        service_was_stopped=true
    fi

    # Atomic move to production location
    if ! mv -- "$BINARY_TMP" "$BINARY_DST"; then
        rm -f -- "$BINARY_TMP"
        if [[ "$service_was_stopped" == true ]]; then
            systemctl --user start "$SERVICE" \
                || log_error "Failed to restart the unchanged ${SERVICE} after install failure"
        fi
        die "Failed to install candidate binary; previous binary retained"
    fi
    log "Deployed: ${BINARY_DST}"
fi

# Restart and liveness check. Any failure after the atomic replacement restores
# the backup before reporting failure.
if [[ "$RESTART" == true ]]; then
    if [[ "$DRY_RUN" == true ]]; then
        log "[dry-run] Would restart ${SERVICE}"
        log "[dry-run] Would run liveness check (${HEALTH_TIMEOUT}s timeout)"
        log "[dry-run] Would auto-rollback on start or liveness failure"
    else
        if ! systemctl --user daemon-reload; then
            log_error "Service reload failed, triggering automatic rollback..."
            if [[ -n "$DEPLOY_BACKUP" ]]; then
                do_rollback "$DEPLOY_BACKUP"
            else
                abort_fresh_deploy "Fresh deploy failed to reload ${SERVICE}"
            fi
            die "Deploy failed. Rolled back to previous version."
        fi
        if ! systemctl --user start "$SERVICE"; then
            log_error "Service restart failed, triggering automatic rollback..."
            if [[ -n "$DEPLOY_BACKUP" ]]; then
                do_rollback "$DEPLOY_BACKUP"
            else
                abort_fresh_deploy "Fresh deploy failed to start"
            fi
            die "Deploy failed. Rolled back to previous version."
        fi
        log "Service started"

        if check_liveness; then
            log "=== Deploy complete ==="
        else
            log "Liveness check failed, triggering automatic rollback..."
            if [[ -n "$DEPLOY_BACKUP" ]]; then
                do_rollback "$DEPLOY_BACKUP"
            else
                abort_fresh_deploy "Fresh deploy was not live"
            fi
            die "Deploy failed. Rolled back to previous version."
        fi
    fi
else
    log "=== Deploy complete (no restart) ==="
fi
