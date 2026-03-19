# R1470: Chiron Self-Auditing via Prosoche Checks

**Date:** 2026-03-19
**Author:** Research agent
**Status:** Final
**Closes:** #1470

---

## Executive Summary

Aletheia's daemon already runs prosoche (directed-attention) checks for calendar events, tasks, and system health. This proposal extends the prosoche system so an agent (specifically the reference "Chiron" configuration, and any agent that opts in) can generate and execute its own audit checks — inspecting its knowledge state, tool usage patterns, session quality, and goal alignment — without human prompting.

**Recommendation: Implement.** The daemon infrastructure (cron scheduling, per-nous task runners) is already in place. Self-auditing is a configuration-level change plus a new audit task category, not a new architectural primitive.

---

## 1. Problem Statement

Today, an agent's self-knowledge degrades silently:

- Facts accumulate without reconciliation (contradictions go unnoticed)
- Instinct patterns drift (tool usage habits that no longer serve the agent's goals persist)
- Session quality is not tracked (the agent cannot detect that its responses have been getting shorter, vaguer, or more uncertain)
- Goal alignment is never verified against GOALS.md (goals can be forgotten or ignored without anyone noticing)

Human operators notice these problems only after they manifest in poor agent behavior. Self-auditing closes this loop: the agent periodically examines its own state, flags anomalies, and either corrects them autonomously or escalates for human review.

The Greek concept "prosoche" (προσοχή) means "attention to oneself" — sustained self-observation. The existing prosoche infrastructure in `daemon` is precisely the hook for this.

---

## 2. Proposed Approach

### 2.1 Audit Check Architecture

A self-audit check is a named, scheduled unit of work that:
1. Queries the agent's own knowledge/session/config state
2. Applies a rule or heuristic
3. Produces a finding (OK / WARNING / CRITICAL) with a description
4. Optionally triggers a remediation action

Checks are defined in `PROSOCHE.md` (already consulted by the bootstrap assembler) with a structured format:

```markdown
## Self-Audit Checks

### knowledge-consistency
schedule: daily
description: Detect contradicting facts and flag for review
action: escalate   # or: auto-correct, suppress

### goal-alignment
schedule: weekly
description: Verify active tool usage aligns with goals in GOALS.md
action: escalate

### session-quality
schedule: daily
description: Detect declining response length or confidence signals
action: report
```

The `daemon` crate reads these check definitions from `PROSOCHE.md` during nous-level task registration.

### 2.2 Audit Check Types

#### 2.2.1 Knowledge Consistency Check

Queries `mneme` for facts flagged with `contradicts` relationships:

```rust
// Find all contradiction pairs
let contradictions = store.find_relationships(RelationshipKind::Contradicts)?;
// Flag pairs where both facts are still active (not retracted/superseded)
let active_contradictions = contradictions
    .filter(|(a, b)| a.is_active() && b.is_active());
```

Finding: list of contradiction pairs with fact IDs, descriptions, and extraction timestamps.
Remediation: for each pair, run LLM consolidation (already exists in `mneme`) or surface to operator via `memory_audit` tool.

#### 2.2.2 Knowledge Staleness Check

Facts have `valid_from`/`valid_to` fields. The staleness check identifies:

- Facts with no update in `N` days where the domain is high-churn (e.g., "current project status")
- Facts whose confidence has decayed below threshold via FSRS

Finding: list of stale facts sorted by staleness × importance.
Remediation: mark as "review needed" tier; surface in next session context.

#### 2.2.3 Goal Alignment Check

Parse GOALS.md goals. Compare against recent tool calls (from session history):

- Which goals have had no related tool invocations in the last 7 days?
- Which tools are being called frequently but map to no stated goal?

This requires a lightweight goal→tool affinity map. For Phase 1, the check asks the LLM to assess alignment using the agent's recent session summaries and GOALS.md content. For Phase 2, a structured affinity matrix (goals × tools) enables quantitative drift detection.

Finding: "Goal X has not been worked on in 14 days" / "Tool Y is used heavily but serves no stated goal."

#### 2.2.4 Session Quality Check

Compute rolling metrics from `mneme`'s session store:

| Metric | Staleness signal |
|---|---|
| Mean response token count | Declining trend → responses getting shorter |
| Tool call rate | Declining → agent becoming passive |
| Clarification request rate | Increasing → agent becoming uncertain |
| Session archive rate | Increasing → sessions being abandoned |

Finding: trend anomalies. Alert if any metric deviates >2σ from the 30-day rolling baseline.

#### 2.2.5 Instinct Pattern Audit

The instinct system captures tool usage patterns. The audit checks:

- Instincts with `confidence < 0.3` (weak signal, should be pruned)
- Instincts that have not been triggered in 30+ days (stale patterns)
- Contradictory instincts (pattern A says "prefer tool X", pattern B says "avoid tool X" in the same context)

Finding: list of instincts flagged for pruning or review.

### 2.3 Scheduling

Self-audit checks run as nous-level background tasks, parallel to the existing `knowledge-maintenance` and `distillation` tasks in `daemon`:

```rust
// crates/daemon/src/maintenance/self_audit.rs
pub struct SelfAuditTask {
    nous_id: NousId,
    checks: Vec<AuditCheck>,
}

impl MaintenanceTask for SelfAuditTask {
    fn name(&self) -> &str { "self-audit" }
    fn schedule(&self) -> CronSchedule { CronSchedule::Daily }
    fn run(&self, ctx: &TaskContext) -> impl Future<Output = Result<()>>
}
```

Checks run sequentially within the task. A check that panics does not cancel subsequent checks (isolated error handling per check).

### 2.4 Finding Storage and Escalation

Findings are stored as a new fact tier: `AuditFinding`. They:
- Appear in `memory_audit` tool output
- Are included in the next session's prosoche section of the system prompt (bootstrap assembler picks up recent findings)
- Trigger an operator notification if `action: escalate` and the finding is CRITICAL

The agent sees its own findings in context at the next session start. It can acknowledge, dismiss, or act on them.

### 2.5 Configuration

```toml
[self_audit]
enabled = true
checks = ["knowledge-consistency", "goal-alignment", "session-quality", "instinct-patterns"]
# Override per-check schedules
[self_audit.overrides.goal-alignment]
schedule = "weekly"
```

Default: enabled for all agents that have a `PROSOCHE.md` with self-audit checks defined. Disabled globally by default until an agent opts in.

---

## 3. Alternatives Considered

### 3.1 Human-Triggered Audits Only

Require the operator to run `aletheia maintenance run self-audit` manually.

**Rejected.** Removes the autonomy benefit. The daemon infrastructure exists precisely to automate recurring tasks.

### 3.2 LLM-Generated Audit Checks

Ask the LLM to generate and run its own audit checks at session start.

**Deferred.** Interesting for Phase 2 (open-ended self-examination). Phase 1 uses structured checks because they are cheaper, deterministic, and auditable.

### 3.3 Separate Audit Agent

Create a second "auditor" nous that monitors the primary agent from the outside.

**Rejected for Phase 1.** Adds significant complexity (inter-nous communication, shared read access to another agent's knowledge store). The self-audit model is simpler and sufficient. The external auditor pattern is worth revisiting for multi-agent deployments.

### 3.4 Rule-Based Only (No LLM)

Run all checks as pure Datalog queries without LLM involvement.

**Partially accepted.** Phase 1 uses Datalog/SQL for knowledge consistency, staleness, and instinct checks. Goal alignment uses a lightweight LLM call. Session quality is purely statistical. This minimizes LLM cost while preserving accuracy for semantically rich checks.

---

## 4. Open Questions

1. **Finding persistence:** How long should audit findings persist before being auto-archived? 30 days? Until acknowledged?

2. **Remediation guardrails:** If a check auto-corrects (e.g., merges contradicting facts), is human review required before the change is committed? The answer should probably be: auto-correct only for `confidence < 0.3` facts; escalate for higher-confidence contradictions.

3. **Circular audit:** If the agent's goal-alignment check flags a goal as unworked, and the agent acts on that finding, is the resulting session attributed to the finding? (Traceability.)

4. **Bootstrap interaction:** Audit findings injected into the system prompt add tokens. How much space should findings get? Priority relative to MEMORY.md and CONTEXT.md?

5. **Agent-authored checks:** Should agents be able to write new audit checks to their own PROSOCHE.md (via `write` tool) during a session? This is the path to fully self-modifying prosoche behavior.

6. **Multi-nous:** In a deployment with 5 agents, do self-audit findings from one agent ever affect another? (e.g., agent A's knowledge inconsistency is a symptom of agent B's incorrect output.)

---

## 5. Implementation Sketch

```
crates/daemon/src/maintenance/
  self_audit.rs           # SelfAuditTask, AuditCheck trait, finding storage

crates/daemon/src/maintenance/checks/
  knowledge_consistency.rs
  knowledge_staleness.rs
  goal_alignment.rs
  session_quality.rs
  instinct_patterns.rs

crates/nous/src/bootstrap/
  prosoche.rs             # extend to include recent AuditFinding facts in system prompt

crates/taxis/src/config.rs
  # SelfAuditConfig struct
```

The PROSOCHE.md format extension is backward-compatible: existing prosoche files without `## Self-Audit Checks` simply run no checks.

---

## 6. References

- Marcus Aurelius, *Meditations* — prosoche as sustained self-attention practice
- Existing prosoche implementation: `crates/daemon/src/maintenance/`
- Bootstrap assembler: `crates/nous/src/bootstrap/`
- Instinct system: `crates/mneme/src/instinct.rs` (inferred)
- Knowledge consistency / consolidation: `crates/mneme/src/knowledge_store/`
