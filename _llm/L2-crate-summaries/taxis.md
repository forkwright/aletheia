# taxis

**Purpose:** Configuration cascade and path resolution for Aletheia.

## Key types

| Type | Purpose |
|------|---------|
| `AletheiaConfig` | Current public type or boundary; see L3/source for exact fields |
| `DaemonBehaviorConfig` | Current public type or boundary; see L3/source for exact fields |
| `ConfigRegistry` | Current public type or boundary; see L3/source for exact fields |
| `HotReloadClass` | Current public type or boundary; see L3/source for exact fields |
| `Oikos` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `taxis::cascade` - public items from `src/cascade.rs`
- `taxis::config/agents` - public items from `src/config/agents.rs`
- `taxis::config/behavior/api` - public items from `src/config/behavior/api.rs`
- `taxis::config/behavior/daemon` - public items from `src/config/behavior/daemon.rs`
- `taxis::config/behavior/dispatch` - public items from `src/config/behavior/dispatch.rs`

## When to look here

- When work touches `crates/taxis` or downstream imports from `taxis`.
- For exact signatures, load `_llm/L3-api-index/taxis.md` if present, then source.

## Recent changes

Schema preflight, daemonBehavior, hot_reloadable registry metadata, and the expanded config registry are current.
