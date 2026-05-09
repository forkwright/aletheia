# melete

**Purpose:** Context distillation engine - compresses conversation history.

## Key types

| Type | Purpose |
|------|---------|
| `Contradiction` | Current public type or boundary; see L3/source for exact fields |
| `ResolutionStrategy` | Current public type or boundary; see L3/source for exact fields |
| `ContradictionLog` | Current public type or boundary; see L3/source for exact fields |
| `empty` | Current public type or boundary; see L3/source for exact fields |
| `is_empty` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `melete::contradiction` - public items from `src/contradiction.rs`
- `melete::distill` - public items from `src/distill.rs`
- `melete::dream` - public items from `src/dream/mod.rs`
- `melete::error` - public items from `src/error.rs`
- `melete::flush` - public items from `src/flush.rs`

## When to look here

- When work touches `crates/melete` or downstream imports from `melete`.
- For exact signatures, load `_llm/L3-api-index/melete.md` if present, then source.

## Recent changes

L3 was refreshed for context distillation after the substrate-wide API update.
