# koilon

**Purpose:** Terminal dashboard for the Aletheia distributed cognition system.

## Key types

| Type | Purpose |
|------|---------|
| `DashboardState` | Current public type or boundary; see L3/source for exact fields |
| `ConnectionState` | Current public type or boundary; see L3/source for exact fields |
| `RenderState` | Current public type or boundary; see L3/source for exact fields |
| `ViewportState` | Current public type or boundary; see L3/source for exact fields |
| `InteractionState` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `koilon::app` - public items from `src/app/mod.rs`
- `koilon::command` - public items from `src/command/mod.rs`
- `koilon::error` - public items from `src/error.rs`
- `koilon::events` - public items from `src/events.rs`
- `koilon::lib` - public items from `src/lib.rs`

## When to look here

- When work touches `crates/theatron/koilon` or downstream imports from `koilon`.
- For exact signatures, load `_llm/L3-api-index/koilon.md` if present, then source.

## Recent changes

L3 was refreshed for the terminal UI crate after shared type movement.
