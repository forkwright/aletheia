# poiesis-inspect

**Purpose:** Text extraction from PDF, XLSX, and PPTX documents.

## Key types

| Type | Purpose |
|------|---------|
| `Result` | Current public type or boundary; see L3/source for exact fields |
| `InspectError` | Current public type or boundary; see L3/source for exact fields |
| `PdfSummary` | Current public type or boundary; see L3/source for exact fields |
| `WorkbookSummary` | Current public type or boundary; see L3/source for exact fields |
| `PresentationSummary` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `poiesis-inspect::error` - public items from `src/error.rs`
- `poiesis-inspect::lib` - public items from `src/lib.rs`

## When to look here

- When work touches `crates/poiesis/inspect` or downstream imports from `poiesis-inspect`.
- For exact signatures, load `_llm/L3-api-index/poiesis-inspect.md` if present, then source.

## Recent changes

The inspection backend is now present in the L3 index.
