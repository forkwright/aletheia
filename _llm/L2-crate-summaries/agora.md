# agora

**Purpose:** Channel registry and provider implementations (Signal).

## Key types

| Type | Purpose |
|------|---------|
| `ChannelProvider` | Current public type or boundary; see L3/source for exact fields |
| `ChannelRegistry` | Current public type or boundary; see L3/source for exact fields |
| `ChannelListener` | Current public type or boundary; see L3/source for exact fields |
| `MessageRouter` | Current public type or boundary; see L3/source for exact fields |
| `SignalProvider` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `agora::error` - public items from `src/error.rs`
- `agora::listener` - public items from `src/listener.rs`
- `agora::metrics` - public items from `src/metrics.rs`
- `agora::registry` - public items from `src/registry.rs`
- `agora::router` - public items from `src/router.rs`

## When to look here

- When work touches `crates/agora` or downstream imports from `agora`.
- For exact signatures, load `_llm/L3-api-index/agora.md` if present, then source.

## Recent changes

Signal capability claims were made honest and listener cleanup now uses registered abort callbacks.
