# Prosoche: directed attention

Prosoche (προσοχή) = directed attention. Checked on each heartbeat tick. Be fast: check and report. Don't investigate.

## Heartbeat checklist

### 1. Instance health
```bash
aletheia health
```
Flag if not healthy.

### 2. Active goals
Scan GOALS.md for anything overdue or blocked.

### 3. Memory hygiene
Is MEMORY.md current? Any stale entries? Any recent session observations that should be recorded?

### 4. Workspace cleanliness
Any temp files, orphaned state, or cruft that accumulated?

## Response format

Nothing needs action:
```
HEARTBEAT_OK
```

Something needs attention: one line per item. No investigation, no research. Just the signal.

## Rules

- 5 tool calls maximum per tick
- Check and report. Do not fix, investigate, or research during heartbeat.
- If a check fails, skip it and note the failure
