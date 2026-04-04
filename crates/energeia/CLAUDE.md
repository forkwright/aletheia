# energeia

Dispatch orchestration: actualization of plans into execution. Absorbs kanon's phronesis dispatch system into aletheia.

## Read first

1. `src/types.rs`: DispatchSpec, DispatchResult, SessionOutcome, Budget, ResumePolicy, QA types
2. `src/engine.rs`: DispatchEngine trait, SessionHandle trait, SessionSpec, AgentOptions
3. `src/qa.rs`: QaGate trait, PromptSpec
4. `src/error.rs`: snafu error enum

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `DispatchSpec` | `types.rs` | What to dispatch: prompt numbers, DAG reference, project |
| `DispatchResult` | `types.rs` | Aggregate outcome: dispatch_id, outcomes, cost, duration |
| `SessionOutcome` | `types.rs` | Per-prompt result: status, cost, turns, duration, PR URL |
| `SessionStatus` | `types.rs` | Terminal state: Success, Failed, Stuck, Aborted, BudgetExceeded |
| `Budget` | `types.rs` | Cost/turn/duration limits with record + check |
| `BudgetStatus` | `types.rs` | Ok, Warning, Exceeded |
| `ResumePolicy` | `types.rs` | Multi-stage escalation with turn budgets |
| `QaResult` | `types.rs` | QA evaluation outcome: verdict, criteria, mechanical issues |
| `QaVerdict` | `types.rs` | Pass, Partial, Fail |
| `DispatchEngine` | `engine.rs` | Trait: spawn/resume sessions against Agent SDK |
| `SessionHandle` | `engine.rs` | Trait: event stream, wait, abort for a running session |
| `QaGate` | `qa.rs` | Trait: evaluate PR quality, mechanical checks |

## Patterns

- **Atomic budget tracking**: Budget uses atomic operations for thread-safe concurrent recording
- **Health-driven escalation**: ResumePolicy defines stages with escalating urgency
- **Mechanical pre-screening**: QaGate separates fast structural checks from LLM evaluation
- **Agent SDK target**: DispatchEngine targets Anthropic Agent SDK HTTP/SSE API

## Common tasks

| Task | Where |
|------|-------|
| Add dispatch type | `src/types.rs` |
| Add error variant | `src/error.rs` |
| Add engine capability | `src/engine.rs` (DispatchEngine or SessionHandle trait) |
| Add QA check | `src/qa.rs` (QaGate trait or MechanicalIssueKind) |

## Dependencies

Uses: koina, eidos, jiff, serde, snafu, tokio (hermeneus pending compilation fix)
Used by: aletheia (binary, behind `energeia` feature flag)
