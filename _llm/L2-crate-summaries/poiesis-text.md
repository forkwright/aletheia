# poiesis-text

**Purpose:** PDF and ODT document rendering backends for poiesis.

## Key types

| Type | Purpose |
|------|---------|
| `OdtError` | Current public type or boundary; see L3/source for exact fields |
| `OdtRenderer` | Current public type or boundary; see L3/source for exact fields |
| `new` | Current public type or boundary; see L3/source for exact fields |
| `PdfError` | Current public type or boundary; see L3/source for exact fields |
| `PdfRenderer` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `poiesis-text::odt` - public items from `src/odt.rs`
- `poiesis-text::pdf` - public items from `src/pdf.rs`

## When to look here

- When work touches `crates/poiesis/text` or downstream imports from `poiesis-text`.
- For exact signatures, load `_llm/L3-api-index/poiesis-text.md` if present, then source.

## Recent changes

PDF/ODT drift fixes are reflected in the refreshed API surface.
