# gnosis

**Purpose:** Machine-derived code-graph index for symbol-level cross-crate queries.

## Key types

| Type | Purpose |
|------|---------|
| `CodeGraph` | Current public type or boundary; see L3/source for exact fields |
| `QueryRow` | Current public type or boundary; see L3/source for exact fields |
| `GnosisError` | Current public type or boundary; see L3/source for exact fields |
| `Result` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `gnosis::error` - public items from `src/error.rs`
- `gnosis::lib` - public items from `src/lib.rs`
- `gnosis::query` - public items from `src/query.rs`

## When to look here

- When work touches `crates/gnosis` or downstream imports from `gnosis`.
- For exact signatures, load `_llm/L3-api-index/gnosis.md` if present, then source.

## Recent changes

Code graph indexing now records SHA-256, module_path, re-exports, and orphan cleanup behavior.
