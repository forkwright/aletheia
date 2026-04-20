# poiesis

## At a glance

Report tooling family: format-agnostic document model plus rendering backends (Typst→PDF, ODT, XLSX, ODS, PPTX) and report-quality checks (prose lint, numeric claim verify). Zero internal dependencies. Entry points: `core/src/lib.rs` (Document, Renderer) and `typst/src/lib.rs` (`render_typst`, `render_template`).

**Typst is the primary renderer.** Prefer `poiesis-typst` for prose-oriented PDF output — it supports templates, math, citations, breakable blocks, cross-references, and JSON data injection, with structured compile diagnostics carrying source locations. The `text/pdf` backend (via `krilla`) remains for the simple document-model path where no template system or data injection is needed.

## Depth

Report tooling family: format-agnostic document model, Typst-based PDF primary renderer, format-specific backends (ODT/XLSX/ODS/PPTX), and report-quality checks (lint + verify). ~2K lines.

## Read first

1. `core/src/lib.rs`: Module structure and public re-exports
2. `core/src/renderer.rs`: The `Renderer` trait
3. `core/src/document.rs`: Top-level `Document` type
4. `core/src/block.rs`: `Block` enum - headings, paragraphs, tables, lists, images, page breaks
5. `text/src/pdf.rs`: Most complex backend (A4 layout, line wrapping, system font loading)

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `render_typst` | `typst/src/lib.rs` | Primary PDF path: Typst source + JSON data → PDF bytes |
| `render_template` | `typst/src/lib.rs` | Resolve a built-in template slug and render |
| `PoiesisError` | `typst/src/error.rs` | Typst render errors (serialize, compile, PDF export, unknown slug) |
| `Document` | `core/src/document.rs` | Format-agnostic document: metadata + ordered content blocks |
| `Renderer` | `core/src/renderer.rs` | Trait: `render(&self, &Document) -> Result<Vec<u8>, Error>` |
| `Metadata` | `core/src/metadata.rs` | Title, author, optional creation timestamp |
| `Block` | `core/src/block.rs` | Block-level element: Heading, Paragraph, Table, List, Image, PageBreak |
| `RichText` | `core/src/rich_text.rs` | Sequence of styled inline `Span`s |
| `Span` | `core/src/rich_text.rs` | Inline style: Plain, Bold, Italic, Code, Link |
| `PdfRenderer` | `text/src/pdf.rs` | Document-model PDF backend via `krilla` (A4, requires system font) |
| `OdtRenderer` | `text/src/odt.rs` | ODT backend via clean-room ZIP/XML |
| `XlsxRenderer` | `sheet/src/xlsx.rs` | Excel backend via `rust_xlsxwriter` |
| `OdsRenderer` | `sheet/src/ods.rs` | ODS backend via `spreadsheet-ods` |
| `PptxRenderer` | `slides/src/pptx.rs` | PowerPoint backend via hand-rolled ZIP/XML emitter |
| `Linter` | `lint/src/lib.rs` | Report prose linter (banned words, citations, structure) |
| `Verifier` | `verify/src/lib.rs` | Numeric-claim verifier (arithmetic eval, cross-claim refs) |

## Template slug convention (poiesis-typst)

Built-in Typst templates live at `typst/templates/<slug>.typ` and are embedded
at compile time. Slug → source mapping is registered in `typst/src/templates.rs`.
Templates load their data payload via `json("data.json")`, which
`render_typst` synthesizes from the caller's JSON `Value`.

Current slugs: `default`.

## Feature flags

| Feature | Crate | Default | Purpose |
|---------|-------|---------|---------|
| `pdf` | `poiesis-text` | yes | PDF output via `krilla` |
| `odt` | `poiesis-text` | yes | ODT output via `zip` |
| `xlsx` | `poiesis-sheet` | yes | Excel output via `rust_xlsxwriter` |
| `ods` | `poiesis-sheet` | yes | ODS output via `spreadsheet-ods` |
| `pptx` | `poiesis-slides` | yes | PPTX output via hand-rolled ZIP/XML emitter |

## Patterns

- **Typst-first**: when rendering a prose-oriented report to PDF, use `poiesis-typst`. Templates and math belong there.
- **Format-agnostic core**: `Document`, `Block`, and `RichText` are plain data types with zero format-specific code (used by non-Typst backends).
- **Trait-based backends**: Each non-Typst subcrate implements `Renderer`; callers pass the same `Document` to any backend.
- **Feature-gated dependencies**: Every backend is behind a Cargo feature so consumers only compile what they need.
- **Plain-text fallback**: Non-Typst backends that cannot natively express a feature (images in PDF/spreadsheets, rich text in XLSX) degrade to plain text or alt text rather than failing.
- **ZIP-based packaging**: ODT, XLSX, ODS, and PPTX are all ZIP archives; tests verify the `PK` magic bytes.
- **In-memory Typst world**: `poiesis-typst` embeds the compiler as a library; source and injected data live in memory (no temp files, no CLI subprocess).

## Common tasks

| Task | Where |
|------|-------|
| Add Typst template | Drop `typst/templates/<slug>.typ`, register in `typst/src/templates.rs` (`SLUGS` + `lookup`) |
| Modify Typst compiler wiring | `typst/src/world.rs` (`TypstWorld`, font discovery) |
| Add block type | `core/src/block.rs` (enum) + every non-Typst backend renderer (match arm) |
| Add inline span style | `core/src/rich_text.rs` (enum) + `text/src/odt.rs` (XML mapping) |
| Add new backend | New subcrate implementing `Renderer` for the format |
| Modify PDF layout (document model path) | `text/src/pdf.rs` (page constants, cursor, `draw_wrapped`) |
| Modify table rendering (non-Typst) | Backend-specific table match arm (e.g. `sheet/src/xlsx.rs`) |

## Dependencies

Uses: jiff, snafu, typst + typst-pdf + comemo + parking_lot (typst backend), krilla (optional), zip (optional), quick-xml (optional), rust_xlsxwriter (optional), spreadsheet-ods (optional)
Used by: organon (as report tools)
