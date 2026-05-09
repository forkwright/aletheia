# poiesis-slides

**Purpose:** PPTX presentation rendering backend for poiesis.

## Key types

| Type | Purpose |
|------|---------|
| `Error` | Current public type or boundary; see L3/source for exact fields |
| `Result` | Current public type or boundary; see L3/source for exact fields |
| `SlideSummary` | Current public type or boundary; see L3/source for exact fields |
| `PresentationSummary` | Current public type or boundary; see L3/source for exact fields |
| `render_pptx` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `poiesis-slides::error` - public items from `src/error.rs`
- `poiesis-slides::lib` - public items from `src/lib.rs`
- `poiesis-slides::pptx` - public items from `src/pptx.rs`

## When to look here

- When work touches `crates/poiesis/slides` or downstream imports from `poiesis-slides`.
- For exact signatures, load `_llm/L3-api-index/poiesis-slides.md` if present, then source.

## Recent changes

Presentation backend API was refreshed with the poiesis cluster.
