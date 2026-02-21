#!/bin/sh
# Loop guard hook handler — reads turn:after payload from stdin
# Writes a sentinel file if tool call count exceeds threshold

THRESHOLD=${LOOP_GUARD_THRESHOLD:-15}
SENTINEL_DIR="/tmp/aletheia-loop-guard"

payload=$(cat)
tool_calls=$(echo "$payload" | grep -o '"toolCalls":[0-9]*' | head -1 | cut -d: -f2)
nous_id=$(echo "$payload" | grep -o '"nousId":"[^"]*"' | head -1 | cut -d'"' -f4)

if [ -z "$tool_calls" ] || [ -z "$nous_id" ]; then
  exit 0
fi

if [ "$tool_calls" -ge "$THRESHOLD" ]; then
  mkdir -p "$SENTINEL_DIR"
  echo "{\"nousId\":\"$nous_id\",\"toolCalls\":$tool_calls,\"timestamp\":\"$(date -Iseconds)\"}" \
    > "$SENTINEL_DIR/$nous_id.sentinel"
fi

exit 0
