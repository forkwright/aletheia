#!/usr/bin/env bash
set -euo pipefail
# Deploy the conduwuit Matrix homeserver on this host.
#
# Usage: scripts/deploy-conduwuit.sh [--server-name NAME] [--dry-run]
#   --server-name NAME  Matrix server name (default: matrix.example.com)
#   --dry-run           Print actions without executing
#
# Effects:
#   1. Pulls the pinned conduwuit container image.
#   2. Generates a random registration token at ${SECRETS_DIR}/conduwuit-registration-token
#      (mode 0600) and seeds it into ${CONDUWUIT_DATA_DIR}/registration_token.
#   3. Creates the ${CONDUWUIT_DATA_DIR} data directory.
#   4. Installs Quadlet unit /etc/containers/systemd/conduwuit.container with the
#      requested server name baked in.
#   5. Reloads systemd, starts conduwuit.service, waits for /_matrix/client/versions
#      to return 200 on 127.0.0.1:6167.
#   6. Prints the registration token and next-step instructions.
#
# Requires: podman (>= 4.4 with Quadlet), systemctl, curl, openssl, sudo.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

SERVER_NAME="matrix.example.com"
DRY_RUN=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --server-name)
            SERVER_NAME="$2"
            shift 2
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        -h|--help)
            sed -n '3,25p' "$0"
            exit 0
            ;;
        *)
            echo "[deploy-conduwuit] ERROR: unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

TEMPLATE="${REPO_ROOT}/scripts/conduwuit.container.template"
UNIT_DST="/etc/containers/systemd/conduwuit.container"
DATA_DIR="${CONDUWUIT_DATA_DIR:-${XDG_STATE_HOME:-${HOME}/.local/state}/conduwuit}"
SECRETS_DIR="${SECRETS_DIR:-${XDG_CONFIG_HOME:-${HOME}/.config}/aletheia/secrets}"
TOKEN_PATH="${TOKEN_PATH:-${SECRETS_DIR}/conduwuit-registration-token}"
SERVICE="conduwuit.service"
HEALTH_URL="http://127.0.0.1:6167/_matrix/client/versions"
HEALTH_TIMEOUT=60

log() {
    printf '[deploy-conduwuit] %s\n' "$*"
}

run() {
    # WHY: takes a single string so we can log the rendered command, with `bash -c`
    # as the evaluator (eval is SC2294-flagged and less predictable under set -e).
    if [[ "${DRY_RUN}" -eq 1 ]]; then
        log "DRY: $*"
    else
        bash -c "$*"
    fi
}

require_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "[deploy-conduwuit] ERROR: required command not found: $1" >&2
        exit 1
    fi
}

require_cmd podman
require_cmd systemctl
require_cmd curl
require_cmd openssl
require_cmd sudo

if [[ ! -f "${TEMPLATE}" ]]; then
    echo "[deploy-conduwuit] ERROR: template not found: ${TEMPLATE}" >&2
    exit 1
fi

# Image pin extracted from the template — single source of truth.
IMAGE="$(awk -F= '/^Image=/ {print $2}' "${TEMPLATE}")"
if [[ -z "${IMAGE}" ]]; then
    echo "[deploy-conduwuit] ERROR: could not extract Image= from ${TEMPLATE}" >&2
    exit 1
fi

log "server name: ${SERVER_NAME}"
log "image: ${IMAGE}"
log "data dir: ${DATA_DIR}"
log "unit dst: ${UNIT_DST}"

# 1. Pull image.
log "pulling container image"
run "podman pull '${IMAGE}'"

# 2. Generate registration token (32-byte base64url, stripped padding).
if [[ ! -f "${TOKEN_PATH}" ]]; then
    log "generating registration token at ${TOKEN_PATH}"
    run "mkdir -p '${SECRETS_DIR}' && chmod 0700 '${SECRETS_DIR}'"
    if [[ "${DRY_RUN}" -eq 0 ]]; then
        openssl rand -base64 32 | tr -d '=+/' | cut -c1-32 >"${TOKEN_PATH}"
        chmod 0600 "${TOKEN_PATH}"
    fi
else
    log "registration token already present: ${TOKEN_PATH}"
fi

# 3. Create data dir and seed the token where conduwuit reads it.
log "preparing data directory ${DATA_DIR}"
run "sudo mkdir -p '${DATA_DIR}'"
run "sudo chmod 0700 '${DATA_DIR}'"

if [[ "${DRY_RUN}" -eq 0 ]]; then
    sudo install -m 0600 "${TOKEN_PATH}" "${DATA_DIR}/registration_token"
else
    log "DRY: sudo install -m 0600 ${TOKEN_PATH} ${DATA_DIR}/registration_token"
fi

# 4. Install Quadlet unit with the requested server name substituted.
log "installing Quadlet unit ${UNIT_DST}"
RENDERED="$(mktemp)"
trap 'rm -f "${RENDERED}"' EXIT
RENDERED_CONTENT="$(<"${TEMPLATE}")"
RENDERED_CONTENT="${RENDERED_CONTENT//__SERVER_NAME__/${SERVER_NAME}}"
RENDERED_CONTENT="${RENDERED_CONTENT//__DATA_DIR__/${DATA_DIR}}"
printf '%s\n' "${RENDERED_CONTENT}" >"${RENDERED}"
run "sudo install -m 0644 '${RENDERED}' '${UNIT_DST}'"

# 5. Reload systemd and start the service.
log "reloading systemd"
run "sudo systemctl daemon-reload"

log "starting ${SERVICE}"
run "sudo systemctl restart '${SERVICE}'"

# 6. Health check.
if [[ "${DRY_RUN}" -eq 0 ]]; then
    log "waiting for ${HEALTH_URL} (timeout ${HEALTH_TIMEOUT}s)"
    deadline=$(( $(date +%s) + HEALTH_TIMEOUT ))
    while true; do
        if curl -fs -o /dev/null -w '%{http_code}' --max-time 3 "${HEALTH_URL}" 2>/dev/null | grep -q '^200$'; then
            log "conduwuit is up"
            break
        fi
        if (( $(date +%s) > deadline )); then
            echo "[deploy-conduwuit] ERROR: ${HEALTH_URL} did not return 200 within ${HEALTH_TIMEOUT}s" >&2
            sudo systemctl status "${SERVICE}" --no-pager || true # WHY: diagnostic only; ignore status failure so we still exit 1
            exit 1
        fi
        sleep 2
    done
else
    log "DRY: would poll ${HEALTH_URL} until 200"
fi

cat <<EOF

conduwuit is running on 127.0.0.1:6167 (server name: ${SERVER_NAME}).

Registration token (also saved to ${TOKEN_PATH}):
EOF
if [[ "${DRY_RUN}" -eq 0 ]]; then
    cat "${TOKEN_PATH}"
    echo
fi

cat <<EOF

Next steps:
  1. Register the first user (operator):
       curl -X POST 'http://127.0.0.1:6167/_synapse/admin/v1/register' \\
            -H 'Content-Type: application/json' \\
            -d "{\\"username\\": \\"operator\\", \\"password\\": \\"CHANGE_ME\\", \\"registration_token\\": \\"\$(cat ${TOKEN_PATH})\\"}"

     Or via conduwuit's register endpoint (API paths depend on the version; see upstream docs).

  2. Point an Element client at http://matrix.example.com:6167.

  3. Follow up with Phase 3 (aletheia matrix init) to provision agent users.
EOF
