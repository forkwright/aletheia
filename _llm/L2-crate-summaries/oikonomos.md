# oikonomos

**Purpose:** Oikonomos (the steward) -- per-nous background task runner, cron scheduling, prosoche attention.

## Key types

| Type | Purpose |
|------|---------|
| `NoopBridge` | Current public type or boundary; see L3/source for exact fields |
| `DaemonBridge` | Current public type or boundary; see L3/source for exact fields |
| `Coordinator` | Current public type or boundary; see L3/source for exact fields |
| `new` | Current public type or boundary; see L3/source for exact fields |
| `max_children` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `oikonomos::bridge/bridge_impl` - public items from `src/bridge/bridge_impl.rs`
- `oikonomos::bridge` - public items from `src/bridge.rs`
- `oikonomos::coordination` - public items from `src/coordination.rs`
- `oikonomos::cron/evolution` - public items from `src/cron/evolution.rs`
- `oikonomos::cron/graph_cleanup` - public items from `src/cron/graph_cleanup.rs`

## When to look here

- When work touches `crates/daemon` or downstream imports from `oikonomos`.
- For exact signatures, load `_llm/L3-api-index/oikonomos.md` if present, then source.

## Recent changes

The daemon crate is indexed under its package name; maintenance persistence and drift detection changed today.
