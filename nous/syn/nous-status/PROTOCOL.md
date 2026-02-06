# Agent Status Protocol

## Overview

All domain agents (Syl, Chiron, Eiron) report status to Syn via standardized files.
Syn checks these during heartbeats and alerts Cody when needed.

## Status File Format

Each agent writes to: `/mnt/ssd/moltbot/clawd/nous-status/{agent-id}.md`

```markdown
# {Agent} Status
*Updated: {ISO timestamp}*

## Health
- **Status:** 🟢 OK | 🟡 Attention | 🔴 Blocked

## Active
- {what's in progress}

## Upcoming (7 days)
- {deadlines, events, commitments}

## Blocked
- {anything stuck, needs input}

## Cross-Domain
- {impacts on other domains}

## Notes
- {anything else Syn should know}
```

## Update Cadence

| Agent | When to Update |
|-------|----------------|
| Syl | After significant household events, weekly minimum |
| Chiron | After work sessions, when projects change status |
| Eiron | After class sessions, when assignments complete/change |

## Alert Conditions (Syn → Cody)

**Immediate alert:**
- 🔴 Blocked status
- Cross-domain conflicts
- Deadline at risk

**Daily digest (if requested):**
- Summary of all agent status
- Upcoming deadlines across domains

**On-demand:**
- Cody asks, Syn summarizes

## Checking Schedule

Syn checks agent status files during heartbeats:
- Every 30 min during active hours
- Aggregates into unified view when Cody asks
