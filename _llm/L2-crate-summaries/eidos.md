# eidos

**Purpose:** Shared knowledge types for the Aletheia memory layer.

## Key types

| Type | Purpose |
|------|---------|
| `Fact` | Current public type or boundary; see L3/source for exact fields |
| `FactProvenance` | Current public type or boundary; see L3/source for exact fields |
| `Visibility` | Current public type or boundary; see L3/source for exact fields |
| `MemoryScope` | Current public type or boundary; see L3/source for exact fields |
| `Provenance` | Current public type or boundary; see L3/source for exact fields |
| `EpistemicTier` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `eidos::bookkeeping` - public items from `src/bookkeeping.rs`
- `eidos::id` - public items from `src/id.rs`
- `eidos::knowledge/architecture_fact` - public items from `src/knowledge/architecture_fact.rs`
- `eidos::knowledge/causal` - public items from `src/knowledge/causal.rs`
- `eidos::knowledge/entity` - public items from `src/knowledge/entity.rs`

## When to look here

- When work touches `crates/eidos` or downstream imports from `eidos`.
- For exact signatures, load `_llm/L3-api-index/eidos.md` if present, then source.

## Recent changes

Provenance/citation shapes were unified; facts now carry visibility and MemoryScope through the knowledge stack, with Reflected tier support.
