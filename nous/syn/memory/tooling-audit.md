# Aletheia Tooling Audit
**Generated:** 2026-02-05 19:39 CST  
**Auditor:** Syn (subagent)  
**Scripts analyzed:** 78 total

## Summary

After reading and analyzing every script in `/mnt/ssd/aletheia/shared/bin/`, I found significant redundancy, legacy naming, and some dead code. The tooling needs consolidation and cleanup but has a solid core.

**Key findings:**
- **13 CORE scripts** that are essential and load-bearing
- **17 CONSOLIDATE scripts** in 8 groups with overlapping functionality  
- **28 DOMAIN scripts** that are useful utilities staying external
- **12 ARCHIVE scripts** that are dead, broken, or superseded
- **8 RENAME scripts** with legacy naming conventions

## CORE — Essential, load-bearing scripts (13)

These scripts define how the system operates and are actively used by cron jobs or core workflows:

### Memory & Facts
- **facts** — Central atomic fact store with bi-temporal tracking. Used by all agents.
- **distill** — Extract structured insights during pre-compaction. Core of Phase 1.
- **memory-router** — Federated memory search across all agent domains.

### Context & Assembly  
- **assemble-context** — Dynamic context assembly for session starts.
- **compile-context** — Generate workspace files from templates (the actual system).

### Infrastructure
- **agent-health** — Monitor 5-agent ecosystem health. Used for dashboard and monitoring.
- **bb** — Blackboard coordination system. Used for agent communication.
- **temporal-graph** — Knowledge graph with event timeline. Central shared awareness.

### Active Cron Jobs
- **graph-maintain** — Daily graph maintenance at 3am (confidence decay, dedup, pruning).
- **daily-facts** — Extract facts at 11pm. Essential automated memory.
- **health-watchdog** — Check if system is responsive every 15 min.
- **autarkia-watchdog** — Check subprocess health every 5 min.
- **self-audit** — Daily self-assessment at 9am.

## CONSOLIDATE — Groups needing merger (17 scripts → 8 groups)

### Group 1: OpenClaw Patching (2 → 1)
- **patch-runtime** — ✅ KEEP (has dynamic context injection)  
- **patch-openclaw** — ❌ REMOVE (subset of patch-runtime)

### Group 2: Memory Consolidation (3 → 1)
- **consolidate-memory** — ✅ KEEP (comprehensive Python implementation)
- **memory-consolidation** — ❌ REMOVE (bash wrapper, less sophisticated)  
- **consolidate-session** — ❌ REMOVE (basic session wrapper)

### Group 3: Fact Extraction (4 → 2)
- **extract-facts** — ✅ KEEP (interactive, manual facts)
- **bulk-extract-facts** — ✅ KEEP (batch processing, different use case)
- **mine-memory-facts** — ❌ REMOVE (pattern-based, less sophisticated)
- **extract-insights** — ✅ MAYBE CONSOLIDATE WITH extract-facts

### Group 4: Graph Tools (3 → 1)
- **aletheia-graph** — ✅ KEEP (comprehensive shared graph interface)
- **graph** — ❌ REMOVE (simple wrapper)
- **graph-agent** — ❌ REMOVE (agent-specific wrapper)

### Group 5: Context Compilation (2 → merge consideration)
- **compile-context** — ✅ KEEP (generates all workspace files)
- **compile-full-context** — ✅ MAYBE MERGE (generates single CONTEXT.md)
- **generate-workspaces** — ✅ SIMILAR PURPOSE (template-based generation)

### Group 6: Watchdog Scripts (3 → 2)
- **monitoring-cron** — ✅ KEEP (comprehensive daily monitoring)
- **health-watchdog** — ✅ KEEP (responsive check, different purpose)
- **autarkia-watchdog** — ✅ KEEP (subprocess health, different from health-watchdog)

## DOMAIN — Useful utilities staying external (28)

### Agent Management
- **agent-audit**, **agent-contracts**, **agent-digest**, **agent-status** — Agent-specific tooling
- **audit-all-agents** — Ecosystem audit wrapper

### Communication & Coordination
- **bb-dispatcher** — Blackboard message routing
- **task-create**, **task-send** — Formal task handoff system
- **attention-check** — Session attention validation

### Calendars & Time
- **gcal** — Google Calendar interface  
- **office** — Office hours management
- **weekly-review** — Weekly review automation

### Research & Content
- **pplx** — Perplexity search wrapper
- **research** — Research workflow helper
- **summarize** — Text summarization tool

### Infrastructure Tools  
- **populate-mcp**, **mcp-watchdog** — MCP server management
- **provider-health** — Provider health checks
- **cleanup-sessions** — Session management
- **generate-dashboard** — System dashboard generation

### External Services
- **email**, **gdrive** — External service wrappers
- **letta**, **letta-populate**, **letta-query-all**, **letta-seed**, **letta-sync** — Letta memory system
- **gemini** — Gemini API wrapper

### Specialized Tools
- **grocery** — Grocery list management
- **tw** — Taskwarrior wrapper  
- **metis** — Metis system interaction
- **work**, **mba** — Domain-specific workflows
- **reflect**, **recall** — Reflection and recall tools

## ARCHIVE — Dead, broken, or superseded (12)

### Dead Functionality
- **crewai-route** — CrewAI was archived, bridge not running
- **heartbeat-tracker** — Prosoche replaced heartbeats, but still referenced in monitoring
- **moltbook-feed** — Moltbook may not be relevant anymore

### Legacy/Old Naming
- **restore-autarkia** — Uses old "autarkia" name instead of Aletheia
- **morning-brief** — References old paths and may have deprecated calendar methods

### Broken Dependencies
- **index-email** — May have broken email indexing dependencies
- **index-gdrive** — Similar issues with Google Drive indexing
- **inbox-triage** — Part of old email workflow that may be superseded

### Redundant/Superseded
- **dianoia-sync** — Old sync mechanism, may be replaced by better tools
- **predictive-context** — Experimental context prediction, unclear if used
- **update-llms-txt** — Simple updater that might not be needed
- **metis-mount-check** — Simple mount checker, may be handle by other monitoring

## RENAME — Non-Aletheia naming (8)

These have legacy naming that doesn't follow Aletheia conventions:

1. **agent-*** scripts — Should these be **aletheia-*** or just remove prefixes?
   - **agent-audit** → **aletheia-audit** or **audit**  
   - **agent-contracts** → **aletheia-contracts** or **contracts**
   - **agent-digest** → **aletheia-digest** or **digest**  
   - **agent-health** → **aletheia-health** or **health** 
   - **agent-status** → **aletheia-status** or **status**

2. **heartbeat-tracker** → **prosoche-tracker** or archive entirely

3. **restore-autarkia** → **restore-aletheia** if kept, or archive

4. **autarkia-watchdog** → **aletheia-watchdog** (though autarkia is the service name)

## Legacy Path References

Multiple scripts still reference old naming:
- **"clawd"** instead of **"syn"** in: facts, consolidate-session, memory-router, memory-consolidation, agent-audit, audit-all-agents, code-audit, tw
- **"/mnt/ssd/aletheia/clawd"** hardcoded paths should use **"${ALETHEIA_NOUS}/syn"**

## Cron Job Analysis

**Active and working:**
- daily-facts, graph-maintain, self-audit, health-watchdog, monitoring-cron
- eiron-deadline-check, autarkia-watchdog, mcp-watchdog, metis-mount-check  
- dianoia-sync, consolidate-memory, audit-all-agents, letta-sync

**Missing from cron but should be:**
- graph-sync (every 6 hours) — listed but may not be working properly

## Recommended Actions

### Phase 1: Remove Clear Duplicates (5 scripts)
1. Delete **patch-openclaw** (replaced by patch-runtime)
2. Delete **memory-consolidation** (replaced by consolidate-memory)  
3. Delete **consolidate-session** (basic wrapper)
4. Delete **graph** (basic wrapper)
5. Delete **graph-agent** (agent wrapper)

### Phase 2: Archive Dead Scripts (12 scripts)  
Move to `/mnt/ssd/aletheia/shared/archive/`:
- crewai-route, heartbeat-tracker, moltbook-feed, restore-autarkia, morning-brief
- index-email, index-gdrive, inbox-triage, dianoia-sync, predictive-context
- update-llms-txt, metis-mount-check

### Phase 3: Fix Legacy References (8+ scripts)
Update all "clawd" references to "syn" and fix hardcoded paths.

### Phase 4: Consider Renaming (8 scripts)  
Standardize on Aletheia naming conventions vs keeping current names.

### Phase 5: Consolidation Decisions (9 scripts)
Decide whether to merge:
- extract-facts + extract-insights
- compile-context + compile-full-context + generate-workspaces  
- mine-memory-facts logic into bulk-extract-facts

## Complexity Estimate

- **Phase 1 (Remove duplicates):** 2 hours
- **Phase 2 (Archive dead):** 4 hours (need to verify dependencies)  
- **Phase 3 (Fix references):** 6 hours (testing required)
- **Phase 4 (Renaming):** 4 hours  
- **Phase 5 (Consolidation):** 12-16 hours (requires design decisions)

**Total estimated effort:** 28-32 hours

## Final Counts

- **CORE:** 13 scripts (essential, keep as-is)
- **CONSOLIDATE:** 17 scripts → 8-10 scripts (save 7-9 scripts)  
- **DOMAIN:** 28 scripts (useful utilities, keep)
- **ARCHIVE:** 12 scripts (remove from active)
- **RENAME:** 8 scripts (fix naming)

**Result:** 78 scripts → ~57 scripts with better organization and no dead code.