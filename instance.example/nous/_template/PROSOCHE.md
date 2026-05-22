# Prosoche: directed attention

Prosoche (προσοχή) = directed attention. This file defines what the agent checks on each heartbeat tick.

## Heartbeat checklist

On each heartbeat tick, execute only the numbered checks below. Stay within 60 seconds and 5 tool calls total. Do not investigate, research, or explore beyond these checks.

## 1. Instance health
```bash
aletheia health
```
Flag if the instance is unhealthy or unreachable.

## 2. Active goals
Scan GOALS.md for anything overdue or blocked.

## 3. Memory hygiene
Is MEMORY.md current? Any stale entries? Any recent session observations that should be recorded?

## 4. Workspace cleanliness
Any temp files, orphaned state, or cruft that accumulated?

## Response format

If nothing needs action:
```text
HEARTBEAT_OK
```

If something needs attention, send a brief alert to the operator. One line per item, no investigation.

## Rules
- Do NOT read other agents' workspaces
- Do NOT run sudo commands
- Do NOT investigate or research - just check and report
- Finish within 60 seconds
- Maximum 5 tool calls per tick
- If a check fails, skip it and note the failure

## Customization

Add or remove checklist items as needed for your deployment. The key constraints:
- **Numbered items only** - the heartbeat cron references this checklist directly
- **Commands, not prose** - each item should have a concrete command to run
- **Hard ceiling on tool calls** - keep the total under 5 to avoid token waste
- **Hard ceiling on time** - keep the tick under 60 seconds
- **Binary response** - either HEARTBEAT_OK or actionable alerts, nothing in between
