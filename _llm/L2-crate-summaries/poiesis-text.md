# poiesis-text

**Purpose:** PDF and ODT rendering backends for the poiesis document family. PDF uses `krilla` (A4 layout, system font); ODT is a clean-room ZIP/XML implementation.

## Key types

| Type | Purpose |
|------|---------|
| `PdfRenderer` | PDF backend: A4 layout, line wrapping, system font loading via `krilla` |
| `OdtRenderer` | ODT backend: ZIP/XML clean-room implementation |

## Public API surface

- `poiesis_text::PdfRenderer` — implements `Renderer`, produces PDF bytes (requires `pdf` feature, default on)
- `poiesis_text::OdtRenderer` — implements `Renderer`, produces ODT bytes (requires `odt` feature, default on)

## When to look here

- When modifying PDF layout behavior (font selection, margins, line wrapping)
- When fixing or extending ODT XML output
