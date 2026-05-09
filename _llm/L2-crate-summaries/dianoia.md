# dianoia

**Purpose:** Planning and project orchestration - multi-phase state machine with workspace persistence.

## Key types

| Type | Purpose |
|------|---------|
| `Error` | Current public type or boundary; see L3/source for exact fields |
| `GateCondition` | Current public type or boundary; see L3/source for exact fields |
| `GateResult` | Current public type or boundary; see L3/source for exact fields |
| `is_pass` | Current public type or boundary; see L3/source for exact fields |
| `PhaseGate` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `dianoia::error` - public items from `src/error.rs`
- `dianoia::gate` - public items from `src/gate.rs`
- `dianoia::handoff` - public items from `src/handoff.rs`
- `dianoia::intent` - public items from `src/intent.rs`
- `dianoia::metrics` - public items from `src/metrics.rs`

## When to look here

- When work touches `crates/dianoia` or downstream imports from `dianoia`.
- For exact signatures, load `_llm/L3-api-index/dianoia.md` if present, then source.

## Recent changes

L3 was refreshed; no major public planning-state contract change was part of the substrate push.
