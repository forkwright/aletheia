# poiesis-typst

**Purpose:** Typst-based PDF rendering backend for poiesis: embeddable compiler, JSON data injection, template assets.

## Key types

| Type | Purpose |
|------|---------|
| `PoiesisError` | Current public type or boundary; see L3/source for exact fields |
| `Result` | Current public type or boundary; see L3/source for exact fields |
| `render_typst` | Current public type or boundary; see L3/source for exact fields |
| `render_template` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `poiesis-typst::error` - public items from `src/error.rs`
- `poiesis-typst::lib` - public items from `src/lib.rs`

## When to look here

- When work touches `crates/poiesis/typst` or downstream imports from `poiesis-typst`.
- For exact signatures, load `_llm/L3-api-index/poiesis-typst.md` if present, then source.

## Recent changes

The Typst backend is now present in the L3 index.
