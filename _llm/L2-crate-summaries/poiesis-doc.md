# poiesis-doc

**Purpose:** DOCX write and inspect backend for poiesis.

## Key types

| Type | Purpose |
|------|---------|
| `Error` | Current public type or boundary; see L3/source for exact fields |
| `Result` | Current public type or boundary; see L3/source for exact fields |
| `DocxSummary` | Current public type or boundary; see L3/source for exact fields |
| `new` | Current public type or boundary; see L3/source for exact fields |
| `render_docx` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `poiesis-doc::error` - public items from `src/error.rs`
- `poiesis-doc::lib` - public items from `src/lib.rs`

## When to look here

- When work touches `crates/poiesis/doc` or downstream imports from `poiesis-doc`.
- For exact signatures, load `_llm/L3-api-index/poiesis-doc.md` if present, then source.

## Recent changes

The DOCX backend is now present in the L3 index.
