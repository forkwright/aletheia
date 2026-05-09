# theatron

**Purpose:** Thin facade re-exporting skene types for external consumers.

## Key types

| Type | Purpose |
|------|---------|
| `DashboardState` | Current public type or boundary; see L3/source for exact fields |
| `ConnectionState` | Current public type or boundary; see L3/source for exact fields |
| `RenderState` | Current public type or boundary; see L3/source for exact fields |
| `ViewportState` | Current public type or boundary; see L3/source for exact fields |
| `InteractionState` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `theatron::koilon/app` - public items from `koilon/src/app/mod.rs`
- `theatron::koilon/command` - public items from `koilon/src/command/mod.rs`
- `theatron::koilon/error` - public items from `koilon/src/error.rs`
- `theatron::koilon/events` - public items from `koilon/src/events.rs`
- `theatron::koilon/lib` - public items from `koilon/src/lib.rs`

## When to look here

- When work touches `crates/theatron` or downstream imports from `theatron`.
- For exact signatures, load `_llm/L3-api-index/theatron.md` if present, then source.

## Recent changes

L3 was refreshed for the presentation facade after shared UI type movement.
