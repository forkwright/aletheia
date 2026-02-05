# Blackboard Architecture Design

*Cross-agent coordination for the Moltbot ecosystem*

## Overview

The blackboard enables agents (Syn, Syl, Chiron, Eiron, Demiurge) to collaborate without direct communication. Agents post to and read from the blackboard; Syn acts as the control component.

## Components

### 1. Blackboard (Shared State)

```
/mnt/ssd/moltbot/shared/blackboard/
├── tasks.jsonl       # Task queue (requests, claims, completions)
├── messages.jsonl    # Inter-agent messages
├── state.json        # Current system state snapshot
└── DESIGN.md         # This file
```

### 2. Knowledge Sources (Agents)

| Agent | Domain | Capabilities |
|-------|--------|--------------|
| Syn | Meta/orchestrator | Coordinates, delegates, monitors |
| Syl | Home/family | Household, calendar, Cooper |
| Chiron | Work | Summus, professional tasks |
| Eiron | School | MBA, academic work |
| Demiurge | Craft | Creative, Ardent, photography |

### 3. Control Component (Syn)

Syn monitors the blackboard and:
- Routes tasks to appropriate agents
- Detects blocked/stuck states
- Escalates to Cody when needed
- Triggers consensus decisions

## Data Structures

### Task Schema
```json
{
  "id": "uuid",
  "type": "request|claim|update|complete|cancel",
  "from": "agent-id",
  "to": "agent-id|any|broadcast",
  "domain": "home|work|school|craft|meta",
  "priority": "low|medium|high|urgent",
  "title": "Brief description",
  "body": "Full details",
  "context": {},
  "status": "pending|claimed|in-progress|blocked|done|cancelled",
  "claimed_by": "agent-id|null",
  "created_at": "ISO timestamp",
  "updated_at": "ISO timestamp",
  "due_at": "ISO timestamp|null"
}
```

### Message Schema
```json
{
  "id": "uuid",
  "from": "agent-id",
  "to": "agent-id|all",
  "type": "info|question|answer|alert|handoff",
  "subject": "Brief subject",
  "body": "Message content",
  "context": {},
  "timestamp": "ISO timestamp",
  "read_by": ["agent-id", ...]
}
```

### State Schema
```json
{
  "last_updated": "ISO timestamp",
  "agents": {
    "syn": {"status": "active", "last_seen": "timestamp"},
    "syl": {"status": "active", "last_seen": "timestamp"},
    ...
  },
  "pending_tasks": 3,
  "active_tasks": 2,
  "blocked_tasks": 0
}
```

## Workflows

### Task Request Flow
1. Agent A posts task to `tasks.jsonl` with `type: request`
2. Syn (or target agent) sees pending task
3. Target agent claims with `type: claim`
4. Agent works, posts `type: update` as needed
5. Agent posts `type: complete` when done

### Cross-Domain Handoff
1. Chiron posts: "Need Eiron to verify MBA deadline for client meeting"
2. Syn routes to Eiron
3. Eiron claims, completes, posts result
4. Chiron reads result from blackboard

### Blocked Task Escalation
1. Agent posts task with `status: blocked`
2. Syn detects during heartbeat
3. Syn alerts Cody with context

## CLI Interface

```bash
# Post a task
bb post "Task title" --to eiron --priority high

# List pending tasks
bb list [--status pending|claimed|blocked]

# Claim a task
bb claim <task-id>

# Update task status
bb update <task-id> --status in-progress --note "Working on it"

# Complete a task
bb complete <task-id> --result "Done, found X"

# Send a message
bb msg "Message" --to chiron

# Read messages
bb inbox [--unread]

# Show blackboard state
bb status
```

## Integration Points

| Existing System | Integration |
|-----------------|-------------|
| agent-status/ | Status files feed into `state.json` |
| facts.jsonl | Task completions can generate facts |
| Letta | Agents can store task context in memory |
| Taskwarrior | Complex tasks create tw entries |
| MCP Memory | Task relationships stored as entities |

## Control Logic (Syn)

During heartbeats, Syn:
1. Reads `tasks.jsonl` for pending/blocked tasks
2. Routes unclaimed tasks to appropriate agents
3. Detects stale claims (>24h without update)
4. Checks for cross-domain dependencies
5. Updates `state.json`
6. Alerts Cody on blocked/urgent items

## Design Principles

1. **Decoupled** — Agents never communicate directly
2. **Async** — No blocking waits, poll-based
3. **Auditable** — All actions logged in JSONL
4. **Recoverable** — Replay from log if needed
5. **Simple** — File-based, no external dependencies

---
*Design based on classic blackboard architecture research + modern multi-agent patterns*
*Created: 2026-01-29*
