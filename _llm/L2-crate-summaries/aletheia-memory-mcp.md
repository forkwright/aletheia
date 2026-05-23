# aletheia-memory-mcp

**Purpose:** Standalone stdio MCP server exposing Aletheia's memory and token-gated write tools to external agents.

## Key types

| Type | Purpose |
|------|---------|
| `Cli` | Current public type or boundary; see L3/source for exact fields |
| `ServerConfig` | Current public type or boundary; see L3/source for exact fields |
| `WriteGate` | Current public type or boundary; see L3/source for exact fields |
| `ToolHandlers` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `aletheia-memory-mcp::error` - public items from `src/error.rs`
- `aletheia-memory-mcp::server` - public items from `src/server.rs`
- `aletheia-memory-mcp::tools` - public items from `src/tools.rs`

## When to look here

- When work touches `crates/aletheia-memory-mcp` or downstream imports from `aletheia-memory-mcp`.
- For exact signatures, load `_llm/L3-api-index/aletheia-memory-mcp.md` if present, then source.

## Recent changes

The standalone MCP surface uses the nous_* namespace, hides write tools unless a per-process capability token is configured, and clearly separates Aletheia local nous memory from kanon mnemosyne.
