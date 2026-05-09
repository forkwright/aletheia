# diaporeia

**Purpose:** MCP server interface - the passage through for external AI agents.

## Key types

| Type | Purpose |
|------|---------|
| `DiaporeiaServer` | Current public type or boundary; see L3/source for exact fields |
| `ServerState` | Current public type or boundary; see L3/source for exact fields |
| `McpClaims` | Current public type or boundary; see L3/source for exact fields |
| `RateLimiter` | Current public type or boundary; see L3/source for exact fields |
| `KnowledgeSearchParams` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `diaporeia::auth` - public items from `src/auth.rs`
- `diaporeia::client` - public items from `src/client.rs`
- `diaporeia::error` - public items from `src/error.rs`
- `diaporeia::rate_limit` - public items from `src/rate_limit.rs`
- `diaporeia::server` - public items from `src/server.rs`

## When to look here

- When work touches `crates/diaporeia` or downstream imports from `diaporeia`.
- For exact signatures, load `_llm/L3-api-index/diaporeia.md` if present, then source.

## Recent changes

The MCP plane gained stdio client bridging, RBAC claim handling, knowledge_search, depth controls, and shared-state boundary documentation.
