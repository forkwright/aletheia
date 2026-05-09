# graphe

**Purpose:** Session persistence layer for Aletheia.

## Key types

| Type | Purpose |
|------|---------|
| `SessionStore` | Current public type or boundary; see L3/source for exact fields |
| `Session` | Current public type or boundary; see L3/source for exact fields |
| `Message` | Current public type or boundary; see L3/source for exact fields |
| `UsageRecord` | Current public type or boundary; see L3/source for exact fields |
| `CleanupExpiredEntries` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `graphe::error` - public items from `src/error.rs`
- `graphe::metrics` - public items from `src/metrics.rs`
- `graphe::portability` - public items from `src/portability.rs`
- `graphe::store/fjall_store` - public items from `src/store/fjall_store.rs`
- `graphe::types` - public items from `src/types.rs`

## When to look here

- When work touches `crates/graphe` or downstream imports from `graphe`.
- For exact signatures, load `_llm/L3-api-index/graphe.md` if present, then source.

## Recent changes

TTL overflow handling and cleanup_expired_entries are wired into the fjall-backed store and maintenance path.
