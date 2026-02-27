# Spec 39: Dianoia Autonomy Gradient — Confidence-Gated Step Execution

**Status:** Draft
**Origin:** Issue #302
**Module:** `dianoia`

---

## Problem

Every Dianoia FSM transition currently requires an explicit tool call. There is no auto-advancement, no confidence threshold, no "skip confirmation on routine steps" logic. For straightforward tasks this creates unnecessary friction — the user must approve each step even when the plan is obvious.

The goal is not full autonomy. It is a gradient: confirm at the right granularity based on task novelty and operator-configured trust level.

## Current Behavior

```
idle → questioning   (explicit: START_QUESTIONING tool call)
questioning → research  (explicit: confirmSynthesis tool call)
research → roadmap   (explicit: completeRequirements tool call)
roadmap → executing  (explicit: advanceToExecution tool call)
executing → verifying  (explicit: advanceToVerification tool call)
verifying → next_phase  (explicit: every phase verified individually)
```

Every arrow requires a conscious agent action. On a 10-phase project, that's 10+ explicit checkpoints.

## Proposed Gradient

Four autonomy levels, configurable per agent in `aletheia.json`:

| Level | Name | Behavior |
|-------|------|---------|
| 0 | `confirm-all` | Current behavior. Every transition requires explicit confirmation. |
| 1 | `confirm-destructive` | Auto-advance read-only phases (research, requirements, roadmap). Confirm before writing files or executing code. |
| 2 | `confirm-novel` | Auto-advance when task type matches a previously successful pattern. Confirm when confidence < threshold. |
| 3 | `confirm-never` | Fully autonomous. Confirm only on explicit BLOCK or error. |

Default: `confirm-destructive` (level 1).

## Dependencies

- Competence model integration (Spec 42) for level 2 confidence scoring
- Provider adapters (Spec 38) for cost-aware auto-advancement decisions

## Phases

1. Config schema: add `autonomyLevel` to agent config
2. FSM auto-advance logic for level 1 (read-only phases skip confirmation)
3. Competence integration for level 2 (confidence-gated)
4. Level 3 implementation with audit trail
5. Per-project autonomy overrides

## Open Questions

- Should autonomy level be per-agent, per-project, or both?
- Rollback mechanism if auto-advanced step produces bad output
- Notification strategy: silent auto-advance vs. "auto-approved" status messages
