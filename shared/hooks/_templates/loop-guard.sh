#!/usr/bin/env bash
set -euo pipefail

THRESHOLD="${LOOP_GUARD_THRESHOLD:-15}"
SENTINEL_DIR="/tmp/aletheia-loop-guard"

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
  printf '{"nousId":"%s","toolCalls":%s,"timestamp":"%s"}\n' \
    "$nous_id" "$tool_calls" "$(date -Iseconds)" \
    > "$SENTINEL_DIR/${nous_id}.sentinel"
fi
