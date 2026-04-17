# poiesis

## At a glance

Document rendering family: format-agnostic model plus PDF, ODT, XLSX, ODS, and PPTX backends. Zero internal dependencies. Entry point: `core/src/lib.rs` (Document, Renderer).

## Depth

Document rendering family: format-agnostic document model plus backends for PDF, ODT, XLSX, ODS, and PPTX. ~1.7K lines.

## Read first

1. `core/src/lib.rs`: Module structure and public re-exports
2. `core/src/renderer.rs`: The `Renderer` trait
3. `core/src/document.rs`: Top-level `Document` type
4. `core/src/block.rs`: `Block` enum — headings, paragraphs, tables, lists, images, page breaks
5. `text/src/pdf.rs`: Most complex backend (A4 layout, line wrapping, system font loading)

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `Document` | `core/src/document.rs` | Format-agnostic document: metadata + ordered content blocks |
| `Renderer` | `core/src/renderer.rs` | Trait: `render(&self, &Document) -> Result<Vec<u8>, Error>` |
| `Metadata` | `core/src/metadata.rs` | Title, author, optional creation timestamp |
| `Block` | `core/src/block.rs` | Block-level element: Heading, Paragraph, Table, List, Image, PageBreak |
| `RichText` | `core/src/rich_text.rs` | Sequence of styled inline `Span`s |
| `Span` | `core/src/rich_text.rs` | Inline style: Plain, Bold, Italic, Code, Link |
| `PdfRenderer` | `text/src/pdf.rs` | PDF backend via `krilla` (A4, requires system font) |
| `OdtRenderer` | `text/src/odt.rs` | ODT backend via clean-room ZIP/XML |
| `XlsxRenderer` | `sheet/src/xlsx.rs` | Excel backend via `rust_xlsxwriter` |
| `OdsRenderer` | `sheet/src/ods.rs` | ODS backend via `spreadsheet-ods` |
| `PptxRenderer` | `slides/src/pptx.rs` | PowerPoint backend via hand-rolled ZIP/XML emitter |

## Feature flags

| Feature | Crate | Default | Purpose |
|---------|-------|---------|---------|
| `pdf` | `poiesis-text` | yes | PDF output via `krilla` |
| `odt` | `poiesis-text` | yes | ODT output via `zip` |
| `xlsx` | `poiesis-sheet` | yes | Excel output via `rust_xlsxwriter` |
| `ods` | `poiesis-sheet` | yes | ODS output via `spreadsheet-ods` |
| `pptx` | `poiesis-slides` | yes | PPTX output via hand-rolled ZIP/XML emitter |

## Patterns

- **Format-agnostic core**: `Document`, `Block`, and `RichText` are plain data types with zero format-specific code.
- **Trait-based backends**: Each subcrate implements `Renderer`; callers pass the same `Document` to any backend.
- **Feature-gated dependencies**: Every backend is behind a Cargo feature so consumers only compile what they need.
- **Plain-text fallback**: Backends that cannot natively express a feature (images in PDF/spreadsheets, rich text in XLSX) degrade to plain text or alt text rather than failing.
- **ZIP-based packaging**: ODT, XLSX, ODS, and PPTX are all ZIP archives; tests verify the `PK` magic bytes.

## Common tasks

| Task | Where |
|------|-------|
| Add block type | `core/src/block.rs` (enum) + every backend renderer (match arm) |
| Add inline span style | `core/src/rich_text.rs` (enum) + `text/src/odt.rs` (XML mapping) |
| Add new backend | New subcrate implementing `Renderer` for the format |
| Modify PDF layout | `text/src/pdf.rs` (page constants, cursor, `draw_wrapped`) |
| Modify table rendering | Backend-specific table match arm (e.g. `sheet/src/xlsx.rs`) |
| Change sheet naming | `sheet/src/xlsx.rs` (`sanitize_sheet_name`) or `sheet/src/ods.rs` (`truncate_sheet_name`) |

## Dependencies

Uses: jiff, snafu, krilla (optional), zip (optional), quick-xml (optional), rust_xlsxwriter (optional), spreadsheet-ods (optional)
Used by: (none yet)
