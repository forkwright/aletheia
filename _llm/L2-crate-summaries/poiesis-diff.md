# poiesis-diff

**Purpose:** Cell-level diff for XLSX and PPTX documents.

## Key types

| Type | Purpose |
|------|---------|
| `Result` | Current public type or boundary; see L3/source for exact fields |
| `DiffError` | Current public type or boundary; see L3/source for exact fields |
| `CellDiff` | Current public type or boundary; see L3/source for exact fields |
| `SlideDiff` | Current public type or boundary; see L3/source for exact fields |
| `diff_workbooks` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `poiesis-diff::error` - public items from `src/error.rs`
- `poiesis-diff::lib` - public items from `src/lib.rs`

## When to look here

- When work touches `crates/poiesis/diff` or downstream imports from `poiesis-diff`.
- For exact signatures, load `_llm/L3-api-index/poiesis-diff.md` if present, then source.

## Recent changes

The document diff helper is now present in the L3 index.
