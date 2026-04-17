# poiesis-slides

**Purpose:** PPTX presentation rendering backend for the poiesis document family, implemented via a hand-rolled ZIP/XML emitter.

## Key types

| Type | Purpose |
|------|---------|
| `PptxRenderer` | PowerPoint backend via hand-rolled ZIP/XML emitter |

## Public API surface

- `poiesis_slides::PptxRenderer` — implements `Renderer`, produces PPTX bytes

## When to look here

- When modifying slide layout, text formatting, or PPTX XML structure
- When adding new slide content types beyond `Block` variants
