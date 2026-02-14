# TOOLS.md - Eiron's Tools

> **Shared tools:** See [TOOLS-INFRASTRUCTURE.md](/mnt/ssd/aletheia/shared/TOOLS-INFRASTRUCTURE.md) for common commands (gcal, gdrive, tw, memory_search, pplx, facts, mcporter).


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

Gold standard at: `/mnt/ssd/aletheia/syn/mba/`

```bash
# Quick access (via Syn's tools)
/mnt/ssd/aletheia/syn/bin/mba status
/mnt/ssd/aletheia/syn/bin/mba prep acf
/mnt/ssd/aletheia/syn/bin/mba tasks
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
/mnt/ssd/aletheia/syn/bin/tw project:mba
/mnt/ssd/aletheia/syn/bin/tw project:capstone
```

## Calendar

Deadlines in Google Calendar (via gcal):
```bash
/mnt/ssd/aletheia/syn/bin/gcal events -c cody.kickertz@gmail.com -d 14
```

## Metis Sync

MBA materials sync from Metis:
```bash
/mnt/ssd/aletheia/syn/bin/mba sync
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

## Memory

Use the `memory_search` tool for semantic recall across local workspace files and long-term extracted memories (shared + domain-specific). Facts are automatically extracted from conversations.
