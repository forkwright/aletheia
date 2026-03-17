#!/usr/bin/env bash
set -euo pipefail
# Health monitor for aletheia. Run via cron or systemd timer.
# Checks service health, token expiry, and logs warnings.
#
# Usage:
#   scripts/health-monitor.sh              # check and log
#   scripts/health-monitor.sh --notify     # check, log, and send Signal on failure
#
# Cron example (every 5 minutes):
#   */5 * * * * /path/to/scripts/health-monitor.sh --notify >> /tmp/aletheia-health.log 2>&1

HEALTH_URL="${ALETHEIA_HEALTH_URL:-http://localhost:18789/api/health}"
METRICS_URL="${ALETHEIA_METRICS_URL:-http://localhost:18789/metrics}"
CRED_FILE="${HOME}/.claude/.credentials.json"
NOTIFY=false
SERVICE="aletheia.service"

[[ "${1:-}" == "--notify" ]] && NOTIFY=true

timestamp() { date -u +"%Y-%m-%dT%H:%M:%SZ"; }

log_ok()   { echo "$(timestamp) OK: $1"; }
log_warn() { echo "$(timestamp) WARN: $1"; }
log_err()  { echo "$(timestamp) ERROR: $1"; }

notify() {
    if $NOTIFY && command -v signal-cli &>/dev/null; then
        signal-cli send -m "aletheia: $1" "${ALETHEIA_NOTIFY_TO:-}" 2>/dev/null || true
    fi
}

# 1. Service running?
if ! systemctl --user is-active "$SERVICE" &>/dev/null; then
    log_err "service not running"
    notify "service not running"
    exit 1
fi

# 2. Health endpoint responding?
health=$(curl -sf --max-time 5 "$HEALTH_URL" 2>/dev/null) || {
    log_err "health endpoint unreachable"
    notify "health endpoint unreachable"
    exit 1
}

status=$(echo "$health" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])" 2>/dev/null)
version=$(echo "$health" | python3 -c "import sys,json; print(json.load(sys.stdin)['version'])" 2>/dev/null)

if [[ "$status" != "healthy" ]]; then
    log_err "unhealthy: $status (v$version)"
    notify "unhealthy: $status (v$version)"
    exit 1
fi

log_ok "healthy v$version"

# 3. Token expiry check
if [[ -f "$CRED_FILE" ]]; then
    remaining=$(python3 -c "
import json, time
d = json.load(open('$CRED_FILE'))
oauth = d.get('claudeAiOauth', {})
exp = oauth.get('expiresAt', 0) / 1000
remaining = (exp - time.time()) / 60
print(f'{remaining:.0f}')
" 2>/dev/null || echo "unknown")

    if [[ "$remaining" != "unknown" ]]; then
        if [[ "$remaining" -lt 30 ]]; then
            log_warn "OAuth token expires in ${remaining}m"
            notify "OAuth token expires in ${remaining}m"
        elif [[ "$remaining" -lt 120 ]]; then
            log_warn "OAuth token expires in ${remaining}m"
        else
            log_ok "OAuth token: ${remaining}m remaining"
        fi
    fi
fi

# 4. Cost tracking (from metrics)
if cost=$(curl -sf --max-time 5 "$METRICS_URL" 2>/dev/null | grep "aletheia_llm_cost_total{" | awk '{print $2}'); then
    if [[ -n "$cost" ]]; then
        log_ok "LLM cost today: \$$cost"
    fi
fi
