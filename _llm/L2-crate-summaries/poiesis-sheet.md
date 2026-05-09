# poiesis-sheet

**Purpose:** XLSX and ODS spreadsheet rendering backends for poiesis.

## Key types

| Type | Purpose |
|------|---------|
| `Error` | Current public type or boundary; see L3/source for exact fields |
| `WorkbookSummary` | Current public type or boundary; see L3/source for exact fields |
| `SheetSummary` | Current public type or boundary; see L3/source for exact fields |
| `render_xlsx` | Current public type or boundary; see L3/source for exact fields |
| `inspect_xlsx` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `poiesis-sheet::error` - public items from `src/error.rs`
- `poiesis-sheet::lib` - public items from `src/lib.rs`
- `poiesis-sheet::ods` - public items from `src/ods.rs`
- `poiesis-sheet::xlsx` - public items from `src/xlsx.rs`

## When to look here

- When work touches `crates/poiesis/sheet` or downstream imports from `poiesis-sheet`.
- For exact signatures, load `_llm/L3-api-index/poiesis-sheet.md` if present, then source.

## Recent changes

XLSX/ODS drift fixes are reflected in the refreshed API surface.
