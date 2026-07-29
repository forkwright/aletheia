#!/usr/bin/env bash
set -euo pipefail

# UNSUPPORTED BEFORE v1.0
# This script is a design template for a planned declarative shell hook.
# The current Aletheia runtime uses in-process turn hooks (crates/nous/src/hooks)
# and does NOT load or execute YAML/shell hook files.

THRESHOLD="${LOOP_GUARD_THRESHOLD:-15}"
# WARNING: the sentinel carries a nous ID and persists, so it belongs in the
# per-user state directory rather than the shared temp root. A fixed name under
# TMPDIR is owned by whichever account creates it first, leaves the nous ID
# world-readable, and is open to symlink attack.
SENTINEL_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/aletheia/loop-guard"

payload=$(cat)
tool_calls=$(printf '%s' "$payload" | grep -o '"toolCalls":[0-9]*' | head -1 | cut -d: -f2)
nous_id=$(printf '%s' "$payload" | grep -o '"nousId":"[^"]*"' | head -1 | cut -d'"' -f4)

if [[ -z "${tool_calls:-}" ]] || [[ -z "${nous_id:-}" ]]; then
  echo "error: missing required fields (toolCalls or nousId) in payload" >&2
  exit 1
fi

if [[ ! "$nous_id" =~ ^[a-zA-Z0-9._-]+$ ]]; then
  exit 1
fi

if [[ "$tool_calls" -ge "$THRESHOLD" ]]; then
  mkdir -p "$SENTINEL_DIR"
  chmod 700 "$SENTINEL_DIR"
  printf '{"nousId":"%s","toolCalls":%s,"timestamp":"%s"}\n' \
    "$nous_id" "$tool_calls" "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" \
    > "$SENTINEL_DIR/${nous_id}.sentinel"
fi
