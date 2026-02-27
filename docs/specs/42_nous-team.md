# Spec 42: Nous Team — Closing the Loop Between Primitives and Autonomous Operation

**Status:** Draft
**Origin:** Issue #260
**Module:** Cross-cutting (dianoia, competence, mneme, reflection)

---

## Problem

Aletheia's stated goal is a domain-specific Nous team that reduces cognitive overhead for both operator and agent, with recursive self-improvement as a measurable KPI. The infrastructure exists — sub-agent roles, melete distillation, prosoche signals, competence model, dianoia FSM, Neo4j graph, tiered recall — but these primitives are not connected into a system that operates autonomously. Every non-trivial task still requires operator direction at each handoff.

## Gap 1: No Closed Feedback Loop

**Current:** Competence scores exist but influence nothing at runtime. Reflection findings sit in workspace files that no system reads automatically. The loop is open.

**Target:**
- Routing uses competence: dianoia reads `competence/model.json` and weights sub-agent selection
- Kritikos flags write back: rejection increments `corrections` for the relevant domain automatically
- Reflection findings trigger actions: stable patterns promoted to MNEME.md automatically

**Scope:** ~300 lines across 4 files, connecting things that already exist.

## Gap 2: No Cross-Agent Task Handoff

**Current:** Agents can message each other but there's no structured task protocol. Handoffs are informal, untracked, and lossy.

**Target:**
- `task-create` / `task-send` as first-class primitives
- Task state machine: created → assigned → in-progress → review → done
- Task context travels with the handoff (not reconstructed from memory)

## Gap 3: No Autonomous Prioritization

**Current:** Prosoche scores signals but doesn't act on them. An agent must be woken, read PROSOCHE.md, and decide what to do. No automatic "this is urgent, start now."

**Target:**
- Prosoche triggers can auto-create dianoia projects for high-urgency signals
- Priority queue: agents process highest-scored signal first when idle
- Operator notification for signals above threshold (not just agent wake)

## Dependencies

- Autonomy gradient (Spec 39) for confidence-gated auto-execution
- Provider adapters (Spec 38) for cost-aware task routing
- Config taxis (Spec 36) for per-agent autonomy config

## Gap 4: No Epistemic Confidence Tiers (from #320)

**Current:** A nous states a queried fact and a field-name inference with the same confidence. The user has no way to distinguish "I ran this query and got 372" from "I inferred this from the column name."

**Three tiers:**

| Tier | Label | Meaning | Example |
|------|-------|---------|---------|
| 1 | **Verified** | Checked against external ground truth | Queried Redshift, read the file, ran the test |
| 2 | **Inferred** | Reasoned from available context | "Health Provider probably means treating physician based on the field name" |
| 3 | **Assumed** | No basis checked, could be wrong | "Standard industry practice is X" without checking |

**Implementation approach:** Start with behavioral norm (AGENTS.md template + system prompt self-check instruction), not code. Inline markers: `[verified: queried 2026-02-27]`, `[inferred: field name pattern]`, `[assumed: not tested]`. Optional later: structured `EpistemicClaim` metadata in turn response for UI rendering.

**Relationship to DATA_REQUEST_PLAYBOOK:** The playbook tells the nous *what* to verify. Confidence tiers mark *whether* it did. They compound.

**Acceptance:** AGENTS.md template updated, system prompt includes self-check, SOUL.md template includes tier norms, at least one domain-specific verification playbook per deployed nous.

## Gap 5: Pressure-Triggered Memory Consolidation (from #312)

**Current:** Nightly reflection cron is clock-based (6am). Loses intra-day context — 15 turns with Akron, 10 with Syn, back to Akron, episodic detail degrades before the cron runs.

**Concept (Letta/MemGPT):** A sleep-time consolidation agent — a separate sub-agent spawn triggered by conversation pressure, not a clock. The primary agent handles conversation; the sleep agent merges fragmented observations, deduplicates memories, reorganizes knowledge structure.

**Trigger conditions (configurable):**
```typescript
const CONSOLIDATION_TRIGGERS = {
  turnsElapsed: 20,         // after N turns in a session
  tokenPressure: 0.75,      // when context utilization hits 75%
  domainSwitch: true,       // when user switches to a different agent
  sessionEnd: true,         // when a session goes idle for > 2 hours
};
```

**Architectural separation:** Sleep agent is a distinct sub-agent spawn using haiku (consolidation is pattern recognition, not reasoning). Receives: last N turns + current MNEME.md + recent distillation priming. Outputs: updated MNEME.md sections, new synthesized Qdrant memories, obsolete memory IDs to retract. Primary nous never sees this work in its context window. Fully async, `consolidation:complete` event emitted.

**Relationship:** Supplements (doesn't replace) nightly reflection cron — reflection handles cross-session analysis, consolidation handles intra-session integration.

## Gap 6: Workspace Hygiene Check (from #322)

**Current:** Workspace files accumulate cruft silently. MNEME.md bloats, TELOS.md has completed goals never archived, orphaned files pile up. The operator discovers this when MNEME.md hits 8k tokens in context breakdown.

**Proposed:** Lightweight session-start check (once per session, not every turn). Surfaces issues as a brief note in the first response. Zero noise on clean workspace.

**Checks:**
- **TELOS staleness:** Completed goals not archived, goals with no activity in 30+ days, contradictory goals
- **MNEME bloat:** Token count > 2k threshold, domain knowledge that belongs in ergon_tools, duplicates/contradictions
- **Orphaned files:** Workspace files not referenced by any standard file list, old drafts, stale exports
- **Skill freshness:** Nous-generated skills unused in 60+ days, skills referencing nonexistent resources

**Output format (only if issues found):**
```
─── workspace health ───────────────────────────────
⚠ TELOS.md: 3 completed goals not yet archived
⚠ MNEME.md: 4.2k tokens — consider pruning domain knowledge to ergon_tools
ℹ 2 orphaned files in workspace (old-analysis.md, draft-query.sql)
────────────────────────────────────────────────────
```

**Config:**
```json
{
  "workspaceHygiene": {
    "enabled": true,
    "checkOnSessionStart": true,
    "mnemeBloatThresholdTokens": 2000,
    "staleGoalDays": 30,
    "unusedSkillDays": 60
  }
}
```

All checks are local file reads + token counting — no LLM call. Nous can offer to fix issues but does not auto-fix without confirmation.

## Gap 7: Agent-Writable Workspace Files (from #316)

**Current:** SOUL.md, IDENTITY.md, MNEME.md are static operator-edited files. Agent knowledge flows through Qdrant — fuzzy, retrieved probabilistically. High-confidence, frequently-relevant knowledge should be in the always-present workspace layer.

**Design — protected vs writable:**

| File | Writable? | Rationale |
|------|-----------|-----------|
| SOUL.md | No | Principles set by operator. Should not drift. |
| IDENTITY.md | Partial — designated sections only | Domain expertise sections can evolve |
| TELOS.md | No | Operator sets purpose |
| MNEME.md | Yes — append only | Agent accumulates knowledge |
| USER.md | No | Operator describes the user |
| CONTEXT.md | Yes | Current working context is agent-owned |

**Tools:**
- `workspace_note` — append timestamped bullet to MNEME.md section (Facts/Patterns/Preferences/Lessons). Duplicate detection via embedding similarity before writing.
- `workspace_update_identity` — update designated `## Agent-Writable` sections of IDENTITY.md only. Everything above that heading is operator-only.

**Guardrails:** SOUL.md write attempts return error. Writes < 500 chars. Max 5 workspace writes per turn. `workspace:written` event emitted for audit trail. `allowWorkspaceWrites` config flag (default: true).

## Phases

1. Competence-aware routing in dianoia sub-agent selection
2. Kritikos → competence model feedback loop
3. Reflection → MNEME.md auto-promotion + agent-writable workspace tools (Gap 7)
4. Structured task handoff protocol
5. Prosoche → dianoia auto-project creation
6. Priority queue for idle agents
7. Epistemic confidence tiers — behavioral norm rollout (Gap 4)
8. Pressure-triggered memory consolidation (Gap 5)
9. Session-start workspace hygiene check (Gap 6)
