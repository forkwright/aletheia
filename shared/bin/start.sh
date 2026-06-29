#!/usr/bin/env bash
set -euo pipefail
# start.sh - Template for a local Aletheia startup helper.
# Copy to your instance host and make executable.
# Store credentials with Aletheia or set ANTHROPIC_API_KEY/ANTHROPIC_AUTH_TOKEN.
# Optional Claude Code import requires CLAUDE_CODE_CREDS.

export ALETHEIA_ROOT="${ALETHEIA_ROOT:-$HOME/aletheia/instance}"
ALETHEIA_ENV_FILE="${ALETHEIA_ENV_FILE:-$ALETHEIA_ROOT/config/env}"
ALETHEIA_BIN="${ALETHEIA_BIN:-aletheia}"
CLAUDE_CREDENTIALS="${CLAUDE_CODE_CREDS:-}"

if [[ -f "$ALETHEIA_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090 # Runtime env-file path is operator-owned.
  . "$ALETHEIA_ENV_FILE"
  set +a
fi

export ALETHEIA_ROOT="${ALETHEIA_ROOT:-$HOME/aletheia/instance}"
ALETHEIA_CREDS="${ALETHEIA_CREDS:-$ALETHEIA_ROOT/config/credentials/anthropic.json}"

# Attempt OAuth token refresh if a token is already stored
if command -v credential-refresh &>/dev/null && [[ -f "$ALETHEIA_CREDS" ]]; then
  has_token=$(python3 -c "
import json, sys
try:
    d = json.load(open(sys.argv[1]))
    print('yes' if d.get('token','').startswith('sk-ant-oat') else 'no')
except Exception:
    print('no')
" "$ALETHEIA_CREDS" 2>/dev/null || echo "no")
  if [[ "$has_token" == "yes" ]]; then
    ALETHEIA_ROOT="$ALETHEIA_ROOT" ALETHEIA_CREDS="$ALETHEIA_CREDS" credential-refresh \
      || echo "warn: credential-refresh failed - proceeding with existing token" >&2
  fi
fi

# Fall back to API key sync from Claude Code credentials only when explicitly configured.
if [[ -n "$CLAUDE_CREDENTIALS" && -f "$CLAUDE_CREDENTIALS" ]]; then
  api_key=$(python3 -c "
import json, sys
d = json.load(open(sys.argv[1]))
print(d.get('apiKey', d.get('primaryApiKey', '')))
" "$CLAUDE_CREDENTIALS" 2>/dev/null || true)  # NOTE: failure is non-fatal here
  if [[ -n "$api_key" && "${api_key:0:11}" == "sk-ant-api0" ]]; then
    mkdir -p "$(dirname "$ALETHEIA_CREDS")"
    python3 -c "
import json, sys
path, key = sys.argv[1], sys.argv[2]
try:
    existing = json.load(open(path))
except Exception:
    existing = {}
if not existing.get('token','').startswith('sk-ant-oat'):
    existing.pop('token', None)
    existing['apiKey'] = key
    existing.setdefault('backupKeys', [])
    json.dump(existing, open(path, 'w'), indent=2)
" "$ALETHEIA_CREDS" "$api_key" 2>/dev/null
    chmod 600 "$ALETHEIA_CREDS"
    echo "API key synced from Claude Code credentials"
  fi
elif [[ -n "$CLAUDE_CREDENTIALS" ]]; then
  echo "warn: CLAUDE_CODE_CREDS set but not readable: $CLAUDE_CREDENTIALS" >&2
fi

if [[ ! -f "$ALETHEIA_CREDS" && -z "${ANTHROPIC_API_KEY:-}" && -z "${ANTHROPIC_AUTH_TOKEN:-}" ]]; then
  echo "error: No credentials found. Run: aletheia init, set ANTHROPIC_API_KEY in ${ALETHEIA_ENV_FILE}, or set CLAUDE_CODE_CREDS for explicit Claude Code import" >&2
  exit 1
fi

export ALETHEIA_MEMORY_USER="${ALETHEIA_MEMORY_USER:-$(whoami)}"
if [[ "$ALETHEIA_BIN" == */* ]]; then
  if [[ ! -x "$ALETHEIA_BIN" ]]; then
    echo "error: Aletheia binary not executable at ${ALETHEIA_BIN}. Set ALETHEIA_BIN or install aletheia on PATH." >&2
    exit 127
  fi
elif ! command -v "$ALETHEIA_BIN" >/dev/null 2>&1; then
  echo "error: Aletheia binary not found on PATH. Set ALETHEIA_BIN or install aletheia on PATH." >&2
  exit 127
fi

exec "$ALETHEIA_BIN" "$@"
