# poiesis-sheet

**Purpose:** XLSX and ODS spreadsheet rendering backends for the poiesis document family.

## Key types

| Type | Purpose |
|------|---------|
| `XlsxRenderer` | Excel backend via `rust_xlsxwriter` |
| `OdsRenderer` | ODS backend via `spreadsheet-ods` |

## Public API surface

- `poiesis_sheet::XlsxRenderer` - implements `Renderer`, produces XLSX bytes
- `poiesis_sheet::OdsRenderer` - implements `Renderer`, produces ODS bytes

## When to look here

- When modifying spreadsheet output formatting or adding table features
- When switching or upgrading the underlying XLSX/ODS dependency
