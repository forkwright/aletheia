# dianoia

Planning and project orchestration: multi-phase state machine with workspace persistence. 5.4K lines.

## Read first

1. `src/project.rs`: Project struct, ProjectMode, lifecycle management
2. `src/state.rs`: ProjectState enum, Transition enum, state machine validation
3. `src/plan.rs`: Plan struct, PlanState, dependency tracking, iteration limits
4. `src/phase.rs`: Phase struct, PhaseState, completion tracking
5. `src/workspace.rs`: ProjectWorkspace (on-disk JSON persistence and directory layout)
6. `src/stuck.rs`: StuckDetector (pattern-based loop detection)

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `Project` | `project.rs` | Top-level project: name, mode, state, phases |
| `ProjectState` | `state.rs` | Lifecycle states: Created -> Questioning -> Researching -> ... -> Complete |
| `Transition` | `state.rs` | Valid state transitions with `transition()` validation |
| `Phase` | `phase.rs` | Grouping of related plans with lifecycle state and completion tracking |
| `Plan` | `plan.rs` | Executable plan: dependencies, iteration limits, blockers |
| `ProjectWorkspace` | `workspace.rs` | On-disk persistence: PROJECT.json, phases/, blockers/, artifacts/ |
| `StuckDetector` | `stuck.rs` | Sliding-window pattern detection: repeated errors, same-args loops, alternating failures |
| `HandoffContext` | `handoff.rs` | Context preserved across distillation/shutdown for continuity |
| `HandoffFile` | `handoff.rs` | Reads/writes `.continue-here.json` and `.continue-here.md` |
| `VerificationResult` | `verify.rs` | Goal-backward verification against phase success criteria |
| `ReconciliationResult` | `reconciler.rs` | Database-vs-filesystem state reconciliation outcome |
| `ResearchLevel` | `research.rs` | Complexity-based research depth: Quick, Standard, Deep |

## Patterns

- **State machine**: `ProjectState::transition()` validates moves; invalid transitions return errors. `Paused` remembers previous state.
- **Three project modes**: Full (all phases), Quick (skip research/scoping), Autonomous (background execution).
- **Workspace layout**: `PROJECT.json` + `phases/` + `.dianoia/blockers/` + `artifacts/`.
- **Stuck detection**: sliding window of tool invocations; detects repeated errors, same-args loops, alternating failures, escalating retries.
- **Handoff protocol**: `.continue-here.json`/`.md` written before context breaks; `detect_orphaned()` finds abandoned handoffs.
- **Verification**: goal-backward tracing matches phase criteria against collected evidence.

## Common tasks

| Task | Where |
|------|-------|
| Add project state | `src/state.rs` (ProjectState enum + Transition enum + transition logic) |
| Add plan feature | `src/plan.rs` (Plan struct, PlanState) |
| Modify stuck detection | `src/stuck.rs` (StuckPattern enum, detection functions) |
| Add verification check | `src/verify.rs` (verify_phase, CriterionInput) |
| Add research domain | `src/research.rs` (ResearchDomain enum) |
| Modify workspace layout | `src/workspace.rs` (WorkspaceLayout, create/save/load) |

## Dependencies

Uses: jiff, ulid, serde, serde_json, snafu, prometheus
Used by: nous, aletheia (binary)
