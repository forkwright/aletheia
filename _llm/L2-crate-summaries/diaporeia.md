# diaporeia

**Purpose:** MCP server interface for external AI agents via the Model Context Protocol. Exposes 15 tools covering sessions, nous, knowledge, config, and health.

## Key types

| Type | Purpose |
|------|---------|
| `DiaporeiaServer` | MCP server: implements `ServerHandler`, holds state + rate limiter + tool router |
| `DiaporeiaState` | Shared state: SessionStore, NousManager, ToolRegistry, Oikos, config |
| `RateLimiter` | Per-session rate limiting with Cheap/Expensive tiers |
| `Error` | Error enum with `Into<rmcp::ErrorData>` conversion |

## Public API surface

- `diaporeia::server` - `DiaporeiaServer`, `DiaporeiaState`
- `diaporeia::transport` - Streamable HTTP router (Axum mount at `/mcp`) and stdio transport
- `diaporeia::tools` - MCP tool implementations grouped by domain

## When to look here

- When adding a new MCP tool exposed to external AI agents
- When modifying MCP transport, rate limiting, or error serialization
