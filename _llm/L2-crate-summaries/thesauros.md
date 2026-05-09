# thesauros

**Purpose:** Domain pack loader - external knowledge, tools, and config overlays.

## Key types

| Type | Purpose |
|------|---------|
| `Error` | Current public type or boundary; see L3/source for exact fields |
| `PackSection` | Current public type or boundary; see L3/source for exact fields |
| `LoadedPack` | Current public type or boundary; see L3/source for exact fields |
| `sections_for_agent_or_domains` | Current public type or boundary; see L3/source for exact fields |
| `domains_for_agent` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `thesauros::error` - public items from `src/error.rs`
- `thesauros::loader` - public items from `src/loader.rs`
- `thesauros::manifest` - public items from `src/manifest.rs`
- `thesauros::tools` - public items from `src/tools/mod.rs`

## When to look here

- When work touches `crates/thesauros` or downstream imports from `thesauros`.
- For exact signatures, load `_llm/L3-api-index/thesauros.md` if present, then source.

## Recent changes

L3 was refreshed for domain pack loading after the substrate-wide API update.
