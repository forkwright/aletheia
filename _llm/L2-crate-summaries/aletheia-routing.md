# aletheia-routing

**Purpose:** Shared routing trait and empirical success-rate storage for dispatch and interactive paths.

## Key types

| Type | Purpose |
|------|---------|
| `Router` | Current public type or boundary; see L3/source for exact fields |
| `NoOpRouter` | Current public type or boundary; see L3/source for exact fields |
| `AfterActionStoreError` | Current public type or boundary; see L3/source for exact fields |
| `RollingStats` | Current public type or boundary; see L3/source for exact fields |
| `success_rate` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `aletheia-routing::lib` - public items from `src/lib.rs`
- `aletheia-routing::store` - public items from `src/store.rs`
- `aletheia-routing::types` - public items from `src/types.rs`

## When to look here

- When work touches `crates/aletheia-routing` or downstream imports from `aletheia-routing`.
- For exact signatures, load `_llm/L3-api-index/aletheia-routing.md` if present, then source.

## Recent changes

Routing claims were aligned with the empirical storage and shared trait surface.
