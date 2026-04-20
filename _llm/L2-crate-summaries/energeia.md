# energeia

**Purpose:** Dispatch orchestration for plan execution - actualizes plans into agent sessions with budget tracking, multi-stage escalation, and QA gating.

## Key types

| Type | Purpose |
|------|---------|
| `DispatchSpec` | What to dispatch: prompt numbers, DAG reference, project |
| `DispatchResult` | Aggregate outcome: dispatch_id, per-prompt outcomes, cost, duration |
| `SessionOutcome` | Per-prompt result: status, cost, turns, duration, PR URL |
| `Budget` | Cost/turn/duration limits with record and check |
| `QaGate` | Trait for QA evaluation: verdict (Pass/Partial/Fail), criteria, mechanical issues |

## Public API surface

- `energeia::engine` - `DispatchEngine` trait, `SessionHandle` trait, `SessionSpec`
- `energeia::types` - `DispatchSpec`, `DispatchResult`, `SessionOutcome`, `Budget`, `ResumePolicy`
- `energeia::qa` - `QaGate` trait, `QaResult`, `QaVerdict`, `PromptSpec`

## When to look here

- When adding dispatch strategies, budget policies, or QA evaluation criteria
- When integrating the dispatch engine with a new session provider
