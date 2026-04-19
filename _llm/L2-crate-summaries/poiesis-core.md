# poiesis-core

**Purpose:** Format-agnostic document model: `Document`, `Block`, and `Renderer` trait. The shared foundation for all poiesis rendering backends.

## Key types

| Type | Purpose |
|------|---------|
| `Document` | Format-agnostic document: metadata + ordered content blocks |
| `Renderer` | Trait: `render(&self, &Document) -> Result<Vec<u8>, Error>` |
| `Block` | Block-level element: Heading, Paragraph, Table, List, Image, PageBreak |
| `RichText` | Sequence of styled inline `Span`s |
| `Metadata` | Title, author, optional creation timestamp |

## Public API surface

- `poiesis_core::document` - `Document`, `Metadata`
- `poiesis_core::block` - `Block` enum for all content block types
- `poiesis_core::renderer` - `Renderer` trait implemented by all backends
- `poiesis_core::rich_text` - `RichText`, `Span` for inline styled content

## When to look here

- When adding a new block type to the document model
- When implementing a new rendering backend (implement `Renderer`)
