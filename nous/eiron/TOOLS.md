# TOOLS.md - Eiron's Tools

> **Shared tools:** See [TOOLS-INFRASTRUCTURE.md](/mnt/ssd/moltbot/shared/TOOLS-INFRASTRUCTURE.md) for common commands (gcal, gdrive, tw, letta, pplx, facts, mcporter).


## Memory System

**Primary**: File-based memory with grep search
```bash
# Search memory system
./search_memory.sh "query_term"
./search_memory.sh "aaron|deadline|phase"

# Memory structure
MEMORY.md                    # Main index
memory/projects/             # Project-specific intelligence  
memory/deadlines/            # Schedules and due dates
memory/decisions/            # Decision log with rationale
memory/teams/               # Team member analysis
```

## MBA System (Local)

Gold standard at: `/mnt/ssd/moltbot/clawd/mba/`

```bash
# Quick access (via Syn's tools)
/mnt/ssd/moltbot/clawd/bin/mba status
/mnt/ssd/moltbot/clawd/bin/mba prep acf
/mnt/ssd/moltbot/clawd/bin/mba tasks
```

| Path | Contents |
|------|----------|
| `sp26/acf/` | Advanced Corporate Finance |
| `sp26/strategic_mgmt/` | Strategic Management |
| `sp26/macro/` | Managerial Macroeconomics |
| `sp26/capstone/` | TBC Capstone project |
| `fa25/` | Fall 2025 reference materials |

## School Google Drive (Read-Only)

```bash
# List school drive
rclone lsd gdrive-school:TEMBA/

# Read a file
rclone cat "gdrive-school:TEMBA/path/to/file"
```

**Account:** cody.kickertz@utexas.edu
**Access:** Read-only (shared team folders)

## Tasks

MBA tasks tracked in Taskwarrior:
```bash
/mnt/ssd/moltbot/clawd/bin/tw project:mba
/mnt/ssd/moltbot/clawd/bin/tw project:capstone
```

## Calendar

Deadlines in Google Calendar (via gcal):
```bash
/mnt/ssd/moltbot/clawd/bin/gcal events -c cody.kickertz@gmail.com -d 14
```

## Metis Sync

MBA materials sync from Metis:
```bash
/mnt/ssd/moltbot/clawd/bin/mba sync
```

Source: `ck@192.168.0.17:~/dianoia/chrematistike/`

## Task Management

**Namespace:** `project:school`

```bash
# Add school task
tw add "description" project:school priority:M

# Subprojects
tw add "..." project:school.acf        # Advanced Corporate Finance
tw add "..." project:school.strategy   # Strategic Management
tw add "..." project:school.capstone   # Capstone project
tw add "..." project:school.macro      # Macroeconomics

# View school tasks
tw project:school
tw project:school due.before:1w
```

**Tags:** +assignment, +exam, +team, +reading, +blocked, +review

## Letta Memory

Agent: eiron-memory (agent-40014ebe-121d-4d0b-8ad4-7fecc528375d)

```bash
# Check status (auto-detects agent from workspace)
letta status

# Store a fact
letta remember "important fact here"

# Query memory
letta ask "what do you know about X?"

# Search archival memory
letta recall "topic"

# View memory blocks
letta blocks

# Use explicit agent
letta --agent eiron status
```
