# Task Contract System Implementation

**Date:** 2026-01-29  
**Status:** ✅ Complete  
**Components:** Schema + CLI tools for inter-agent task coordination

## Overview

Implemented a comprehensive task contract system for structured handoffs between domain agents (Syn, Chiron, Eiron, Syl, Demiurge). Based on requirements from research-coordination.md.

## Components Created

### 1. JSON Schema (`/mnt/ssd/moltbot/shared/schemas/task-contract.json`)

**Core contract structure:**
- `task_id` (UUID) - unique identifier
- `correlation_id` - trace across agent boundaries  
- `source_agent` / `target_agent` - routing info
- `task_type` - categorization (query|analysis|synthesis|coordination|execution|monitoring|escalation)
- `context` - full state data including description, dependencies, resource requirements
- `priority` - high|medium|low
- `callback` - how to report results back
- Status tracking - pending → accepted → in_progress → completed/failed

**Key features implemented:**
- ✅ **Resource leasing awareness** - `context.resources_needed[]` with access levels and lease durations
- ✅ **SLA monitoring** - `sla` object with max_duration, progress_interval, escalation_threshold
- ✅ **Explicit state** - Full context preservation with dependencies and state objects
- ✅ **Correlation tracking** - Trace IDs for cross-agent coordination

**Status lifecycle:** pending → accepted → in_progress → blocked/completed/failed/cancelled/escalated

### 2. Task Creation CLI (`/mnt/ssd/moltbot/shared/bin/task-create`)

**Features:**
- Command-line interface for creating task contracts
- Interactive mode (`-i`) for guided creation
- Validation of agent names, task types, priorities
- Auto-generation of UUIDs, timestamps, correlation IDs
- Default SLA values (1hr max, 15min progress updates, 2hr escalation)

**Usage examples:**
```bash
# Basic task
task-create -s syn -t chiron -T analysis -d "Analyze Q4 performance"

# With deadline and callback
task-create -s syn -t eiron -T query -d "Research deadlines" \
    -D "2024-12-15T17:00:00Z" -C "session_message:syn"

# Interactive mode
task-create -i
```

### 3. Task Transmission CLI (`/mnt/ssd/moltbot/shared/bin/task-send`)

**Features:**
- Sends task contracts to target agents via `sessions_send`
- Target agent availability checking (unless `--force`)
- Dry-run mode for testing (`--dry-run`)
- Automatic file management (pending → sent directories)
- Transmission logging
- Rich message formatting with contract details

**Workflow:**
1. Validates task file JSON
2. Checks target agent availability 
3. Sends formatted message with full contract
4. Copies to target agent's pending queue
5. Moves original to sent archive
6. Logs transmission event

## File Structure Created

```
/mnt/ssd/moltbot/shared/
├── schemas/
│   └── task-contract.json          # JSON Schema definition
├── bin/
│   ├── task-create                 # CLI to create contracts  
│   └── task-send                   # CLI to send contracts
└── task-contracts/
    ├── pending/
    │   ├── chiron/                 # Per-agent pending queues
    │   ├── eiron/
    │   ├── syl/
    │   └── demiurge/
    ├── sent/                       # Sent contract archive
    └── transmission.log            # Send/receive log
```

## Integration Points

**With existing systems:**
- Uses `sessions_send` for inter-agent messaging
- Integrates with existing agent session keys (`agent:{name}:main`)
- Follows shared infrastructure patterns at `/mnt/ssd/moltbot/shared/`
- Compatible with current agent naming (syn, chiron, eiron, syl, demiurge)

**Missing for full implementation:**
- `task-respond` CLI for agents to accept/reject tasks
- `task-status` CLI for progress updates  
- `task-complete` CLI for marking completion
- Integration with existing blackboard system
- SLA monitoring daemon for escalation

## Design Decisions

**Resource leasing approach:**
- Resources specified with type, ID, access level, duration
- No actual enforcement yet - schema provides structure for future implementation
- Types: file, api, service, tool, memory, external

**SLA monitoring structure:**
- ISO 8601 durations for all time fields
- Progress reporting interval separate from escalation threshold
- Default values: 1hr max execution, 15min progress updates, 2hr escalation

**Callback flexibility:**
- Multiple callback methods: session_message, file_write, blackboard, direct_call
- Configurable format: json, markdown, plaintext
- Target specification varies by method

**Status granularity:**
- 8 status states covering full lifecycle including blocked and escalated
- Timestamp tracking for all major transitions
- Progress percentage + description for ongoing work

## Next Steps

To complete the task coordination system:

1. **Agent response tooling** - CLIs for accept/reject/update/complete
2. **SLA monitoring** - Background process to check deadlines and escalate
3. **Resource leasing** - Actual enforcement of resource access controls  
4. **Integration** - Connect with blackboard and existing task systems
5. **Metrics** - Task completion rates, SLA adherence, bottlenecks

The foundation is now in place for structured inter-agent coordination with explicit state management and SLA monitoring capabilities.