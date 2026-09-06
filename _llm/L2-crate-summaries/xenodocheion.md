# xenodocheion

**Purpose:** Standalone stdio MCP server exposing Aletheia's memory and token-gated write tools to external agents.

## Key types

| Type | Purpose |
|------|---------|
| `Cli` | Current public type or boundary; see L3/source for exact fields |
| `ServerConfig` | Current public type or boundary; see L3/source for exact fields |
| `WriteGate` | Current public type or boundary; see L3/source for exact fields |
| `ToolHandlers` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `xenodocheion::error` - public items from `src/error.rs`
- `xenodocheion::server` - public items from `src/server.rs`
- `xenodocheion::tools` - public items from `src/tools.rs`

## When to look here

- When work touches `crates/xenodocheion` or downstream imports from `xenodocheion`.
- For exact signatures, load `_llm/L3-api-index/xenodocheion.md` if present, then source.

## Recent changes

The standalone MCP surface uses the nous_* namespace, hides write tools unless a per-process capability token is configured, and clearly separates Aletheia local nous memory from kanon mnemosyne.
