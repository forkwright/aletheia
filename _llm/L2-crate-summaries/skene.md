# skene

**Purpose:** Shared API client, types, SSE, and streaming infrastructure for Aletheia UIs.

## Key types

| Type | Purpose |
|------|---------|
| `ApiClient` | Current public type or boundary; see L3/source for exact fields |
| `new` | Current public type or boundary; see L3/source for exact fields |
| `token` | Current public type or boundary; see L3/source for exact fields |
| `raw_client` | Current public type or boundary; see L3/source for exact fields |
| `SseConnection` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `skene::api/client` - public items from `src/api/client.rs`
- `skene::api/sse` - public items from `src/api/sse.rs`
- `skene::api/streaming` - public items from `src/api/streaming.rs`
- `skene::api/types` - public items from `src/api/types/mod.rs`
- `skene::api/types/verification` - public items from `src/api/types/verification.rs`

## When to look here

- When work touches `crates/theatron/skene` or downstream imports from `skene`.
- For exact signatures, load `_llm/L3-api-index/skene.md` if present, then source.

## Recent changes

L3 was refreshed for shared UI client/SSE types.
