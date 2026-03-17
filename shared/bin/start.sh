#!/usr/bin/env bash
set -euo pipefail
# start.sh — Template for ~/.aletheia/start.sh
# Copy to ~/.aletheia/start.sh and make executable.
# Run `claude setup-token` first to get a 1-year OAuth token, then this script
# uses credential-refresh to keep it renewed automatically on every startup.

ALETHEIA_CREDS="$HOME/.aletheia/credentials/anthropic.json"
CLAUDE_JSON="$HOME/.claude.json"

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
    credential-refresh || echo "warn: credential-refresh failed — proceeding with existing token" >&2
  fi
fi

# Fall back to API key sync from Claude Code config if no OAuth token present
if [[ -f "$CLAUDE_JSON" ]]; then
  api_key=$(python3 -c "
import json, sys
print(json.load(open(sys.argv[1])).get('primaryApiKey', ''))
" "$CLAUDE_JSON" 2>/dev/null || true)
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
    echo "API key synced from Claude Code config"
  fi
fi

if [[ ! -f "$ALETHEIA_CREDS" ]]; then
  echo "error: No credentials found. Run: claude setup-token" >&2
  exit 1
fi

export ALETHEIA_ROOT="${ALETHEIA_ROOT:-$HOME/.aletheia}"
export ALETHEIA_MEMORY_USER="${ALETHEIA_MEMORY_USER:-$(whoami)}"
exec "${ALETHEIA_ROOT}/target/release/aletheia" "$@"
