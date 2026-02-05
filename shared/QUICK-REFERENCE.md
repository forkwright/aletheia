# Quick Reference Card
*All agents: symlink this to your workspace*

## Memory Commands

```bash
# Facts
facts about the operator                    # Facts about a subject
facts search "query"                # Search facts
facts add subj pred obj             # Add fact
facts reinforce <id> -e "evidence"  # Increase confidence
facts contradict <id> -e "evidence" # Decrease confidence
facts conflicts                     # Find contradictions
facts review                        # Low-confidence facts

# Entity Pages
reflect --all                       # Generate all entity pages
reflect --entity the operator               # Generate specific page
reflect --stats                     # Fact statistics

# Federated Search
memory-router "query"               # Auto-routes by domain
memory-router "query" --domains all # Search everywhere
```

## Calendar & Tasks

```bash
# Calendar
gcal today                          # Today's events
gcal events -c CAL_ID -d 7          # Next 7 days
gcal calendars                      # List calendars

# Tasks (Taskwarrior)
tw                                  # Show next actions
tw add "desc" project:X priority:H  # Add task
tw done ID                          # Complete task
tw today                            # Due today
```

## Communication

```bash
# Email
himalaya list                       # List inbox
himalaya read <id>                  # Read message
himalaya write                      # Compose

# Research
pplx "query"                        # Perplexity search
research "query" --sources          # With citations
```

## Agent Coordination

```bash
# Blackboard
bb status                           # Overview
bb post "task" --to agent           # Post task
bb claim <id>                       # Claim task
bb complete <id> "result"           # Complete task

# Agent Health
agent-health                        # Ecosystem health
agent-status                        # All agent statuses
```

## System

```bash
# Provider Health
provider-health                     # Check all LLM providers
provider-health check anthropic     # Check specific

# Infrastructure
laptop ssh                           # SSH to laptop
laptop claude                        # Run Claude on Laptop
ssh nas                             # SSH to NAS
```

## Model Failover (Automatic)

```
Primary: claude-opus-4-5
  ↓ (on failure)
Fallback 1: claude-sonnet-4
  ↓ (on failure)
Fallback 2: claude-haiku-3.5
```

Automatic exponential backoff cooldowns. No manual intervention needed.

## Memory Flush (Automatic)

When session approaches context limit, Clawdbot triggers silent turn to save important context. Enabled automatically.

## Key Locations

| What | Where |
|------|-------|
| Shared tools | `/mnt/ssd/moltbot/shared/bin/` |
| Facts | `memory/facts.jsonl` |
| Entity pages | `memory/entities/*.md` |
| Daily logs | `memory/YYYY-MM-DD.md` |
| Long-term | `MEMORY.md` |
| Full docs | `llms-full.txt` |

---
*Updated: 2026-02-03*
