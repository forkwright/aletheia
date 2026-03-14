# Prosoche: directed attention

Prosoche (προσοχή) = directed attention. This file defines what the agent checks on each heartbeat tick.

## Heartbeat checklist

On each heartbeat tick, execute **only** the numbered items below. Do not investigate, research, or explore beyond these checks.

### 1. Calendar (next 4 hours)
```bash
# Replace with your calendar command(s)
gcal today -c your@calendar.id
```
Flag anything starting within 4 hours that the operator might not be aware of.

### 2. Tasks due today
```bash
# Replace with your task manager command
tw
```
Flag any tasks due today or overdue.

### 3. System health
```bash
nous-health
```
Flag any agent that's unhealthy or unreachable.

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
- Maximum 5 tool calls per tick
- If a check fails, skip it and note the failure

## Customization

Add or remove checklist items as needed for your deployment. The key constraints:
- **Numbered items only** - the heartbeat cron references this checklist directly
- **Commands, not prose** - each item should have a concrete command to run
- **Hard ceiling on tool calls** - keep the total under 5 to avoid token waste
- **Binary response** - either HEARTBEAT_OK or actionable alerts, nothing in between
