# Aletheia Build — Rescue Document
*Final update: 2026-02-05 14:30 CST*

## What Exists Now

### Core Scripts (shared/bin/)
| Script | Purpose |
|---|---|
| `distill` | Pre-compaction: extract facts → facts.jsonl + FalkorDB graph + session state |
| `assemble-context` | Session start: compile state + facts + tasks + graph + calendar |
| `compile-context` | Generate AGENTS.md from templates for all/one agent |
| `generate-tools-md` | Generate lean TOOLS.md from tools.yaml |
| `aletheia-graph` | Shared knowledge graph CLI (add/query/recent/stats/connections) |

### Data Stores
| Store | Location | Content |
|---|---|---|
| facts.jsonl | shared/memory/facts.jsonl | 308+ structured facts |
| FalkorDB "aletheia" graph | docker:falkordb | 15 nodes, 8 edges (new, growing) |
| FalkorDB "temporal_events" | docker:falkordb | 198 nodes (legacy) |
| Session state | nous/*/memory/session-state.yaml | Per-nous focus, open threads |
| Daily memory | nous/*/memory/YYYY-MM-DD.md | 46 files |
| MEMORY.md | nous/*/MEMORY.md | Curated long-term |
| Letta | docker:letta (port 8283) | 5 agent memory stores |

### Template System (shared/templates/)
- sections/: 10 shared markdown sections
- agents/: 7 YAML configs (one per nous)
- All include distillation + context assembly instructions

### Token Savings
- Syn AGENTS.md: 19,682 → 6,763 bytes (66%)
- Syn TOOLS.md: 27,601 → 1,858 bytes (93%)
- Total: ~80% reduction in static context per turn

### OpenClaw Config Changes
- memoryFlush: upgraded to trigger structured distillation
- softThresholdTokens: 6000 → 8000 (earlier trigger)
- PATH includes shared/bin/

### Remaining Tasks
```
tw project:aletheia
```
7 tasks remaining (metaxynoesis research, concept audit, runtime layer design,
SOUL.md audit, token optimization, OpenClaw fork, repo init)

### Build Plan Phases
1. ✅ Distillation — Pre-compaction insight extraction
2. ✅ Context compilation — Dynamic assembly from state+facts+graph
3. ✅ Shared awareness — FalkorDB graph as shared substrate
4. ⬜ Attention system — Adaptive, not periodic
5. ⬜ OpenClaw patches — Make runtime serve Aletheia
6. ⬜ Character refinement — SOUL.md audit, evidence-based

### Architecture
See: nous/syn/docs/aletheia-architecture.md
See: nous/syn/docs/aletheia-plan.md

## Phase 4: Attention System (DONE)
- `attention-check` — adaptive awareness replacing static heartbeat
  - Checks: overdue tasks, calendar, system health, docker, disk, memory, agents, blackboard
  - Time-aware: morning brief, evening wrap, weekend mode
  - Outputs nothing if nothing needs attention (true silence)
  - Confidence decay + reinforcement on graph (daily cron at 3am)
  - `graph-maintain` — self-cleaning graph: decay, dedup, prune
## Phase 5: OpenClaw Patches (DONE)

### Workspace patch applied
- File: /usr/lib/node_modules/openclaw/dist/agents/workspace.js
- Backup: workspace.js.aletheia-backup
- Effect: If CONTEXT.md exists in workspace → load only SOUL.md + CONTEXT.md + MEMORY.md
- Otherwise: default behavior preserved (backward compatible)
- Reapply after updates: `patch-openclaw` (updated to include this)

### compile-full-context
- Generates single CONTEXT.md per agent from AGENTS.md + TOOLS.md + USER.md + IDENTITY.md + HEARTBEAT.md
- Enabled on 6 agents (disabled on syn for safety during active session)

### patch-openclaw updated
- Now applies both: signal group ID fix + workspace dynamic context
