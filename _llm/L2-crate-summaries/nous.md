# nous

**Purpose:** Agent session pipeline - bootstrap, routing, tool execution.

## Key types

| Type | Purpose |
|------|---------|
| `NousActor` | Current public type or boundary; see L3/source for exact fields |
| `NousManager` | Current public type or boundary; see L3/source for exact fields |
| `BootstrapSlot` | Current public type or boundary; see L3/source for exact fields |
| `Step` | Current public type or boundary; see L3/source for exact fields |
| `WorkingState` | Current public type or boundary; see L3/source for exact fields |
| `LoopDetector` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `nous::actor` - public items from `src/actor/mod.rs`
- `nous::actor/spawn` - public items from `src/actor/spawn.rs`
- `nous::adapters` - public items from `src/adapters.rs`
- `nous::audit` - public items from `src/audit.rs`
- `nous::bootstrap` - public items from `src/bootstrap/mod.rs`

## When to look here

- When work touches `crates/nous` or downstream imports from `nous`.
- For exact signatures, load `_llm/L3-api-index/nous.md` if present, then source.

## Recent changes

Working-memory injection, Composite LoopGuard use, BootstrapSlot, Step compaction, tool-group gating, mistake brake, and per-stage timeouts are live.
