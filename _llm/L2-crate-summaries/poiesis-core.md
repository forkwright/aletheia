# poiesis-core

**Purpose:** Format-agnostic document model and Renderer trait for poiesis.

## Key types

| Type | Purpose |
|------|---------|
| `Block` | Current public type or boundary; see L3/source for exact fields |
| `Table` | Current public type or boundary; see L3/source for exact fields |
| `ListItem` | Current public type or boundary; see L3/source for exact fields |
| `Image` | Current public type or boundary; see L3/source for exact fields |
| `Document` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `poiesis-core::block` - public items from `src/block.rs`
- `poiesis-core::document` - public items from `src/document.rs`
- `poiesis-core::metadata` - public items from `src/metadata.rs`
- `poiesis-core::renderer` - public items from `src/renderer.rs`
- `poiesis-core::rich_text` - public items from `src/rich_text.rs`

## When to look here

- When work touches `crates/poiesis/core` or downstream imports from `poiesis-core`.
- For exact signatures, load `_llm/L3-api-index/poiesis-core.md` if present, then source.

## Recent changes

L3 was refreshed for the document model used by the poiesis backends.
