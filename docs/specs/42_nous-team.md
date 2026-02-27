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

## Phases

1. Competence-aware routing in dianoia sub-agent selection
2. Kritikos → competence model feedback loop
3. Reflection → MNEME.md auto-promotion
4. Structured task handoff protocol
5. Prosoche → dianoia auto-project creation
6. Priority queue for idle agents
