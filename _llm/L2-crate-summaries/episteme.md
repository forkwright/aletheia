# episteme

**Purpose:** Knowledge pipeline for Aletheia.

## Key types

| Type | Purpose |
|------|---------|
| `KnowledgeStore` | Current public type or boundary; see L3/source for exact fields |
| `TraceIngestLayer` | Current public type or boundary; see L3/source for exact fields |
| `SideRanker` | Current public type or boundary; see L3/source for exact fields |
| `RecallEngine` | Current public type or boundary; see L3/source for exact fields |
| `QueryRewriteConfig` | Current public type or boundary; see L3/source for exact fields |
| `Visibility` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `episteme::admission` - public items from `src/admission.rs`
- `episteme::bookkeeping/gliner` - public items from `src/bookkeeping/gliner.rs`
- `episteme::bookkeeping` - public items from `src/bookkeeping/mod.rs`
- `episteme::causal` - public items from `src/causal.rs`
- `episteme::conflict` - public items from `src/conflict.rs`

## When to look here

- When work touches `crates/episteme` or downstream imports from `episteme`.
- For exact signatures, load `_llm/L3-api-index/episteme.md` if present, then source.

## Recent changes

TraceIngestLayer, SideRanker, query rewrite, tiered search, and Visibility schema v11 are wired into the knowledge pipeline.
