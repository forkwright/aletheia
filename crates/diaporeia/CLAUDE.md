# diaporeia

## At a glance

MCP server interface for external AI agents. Depends on koina, taxis, nous, and mneme. Entry point: `src/lib.rs` (DiaporeiaServer, DiaporeiaState).

## Depth

MCP server interface for external AI agents via the Model Context Protocol. 1.5K lines.

## Read first

1. `src/server.rs`: DiaporeiaServer (rmcp ServerHandler with tool_router and resources)
2. `src/state.rs`: DiaporeiaState (shared Arc pointers to NousManager, SessionStore, ToolRegistry)
3. `src/transport.rs`: Streamable HTTP router (Axum mount at `/mcp`) and stdio transport
4. `src/tools/mod.rs`: MCP tool implementations (session, nous, knowledge, config, health)
5. `src/error.rs`: Error -> rmcp::ErrorData conversion with path sanitization

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `DiaporeiaServer` | `server.rs` | MCP server: implements `ServerHandler`, holds state + rate limiter + tool router |
| `DiaporeiaState` | `state.rs` | Shared state: SessionStore, NousManager, ToolRegistry, Oikos, config |
| `RateLimiter` | `rate_limit.rs` | Per-session rate limiting with Cheap/Expensive tiers |
| `Error` | `error.rs` | Error enum with `Into<rmcp::ErrorData>` conversion |

## MCP tools (15)

| Tool | Tier | RBAC | Purpose |
|------|------|------|---------|
| `session_create` | Expensive | Operator | Create a new session for a nous agent |
| `session_list` | Cheap | Agent | List sessions, optionally filtered by nous ID |
| `session_message` | Expensive | Operator | Send a message and get the response |
| `session_history` | Cheap | Agent | Get conversation history for a session |
| `nous_list` | Cheap | Agent | List all registered nous agents |
| `nous_status` | Cheap | Agent | Detailed status of a specific agent |
| `nous_tools` | Cheap | Agent | List tools available to an agent |
| `knowledge_search` | Expensive | Operator | Semantic search across the knowledge graph (stub) |
| `knowledge_recall` | Expensive | Agent | BM25 text recall across the knowledge graph |
| `knowledge_get` | Cheap | Agent | Retrieve a single fact by ID |
| `knowledge_insert` | Expensive | Operator | Insert a new fact (returns fact ID) |
| `knowledge_forget` | Expensive | Operator | Soft-delete a fact by ID |
| `knowledge_graph_neighbors` | Expensive | Agent | Traverse entity edges up to configured depth |
| `config_get` | Cheap | Operator | Runtime config (redacted) |
| `system_health` | Cheap | Agent | Uptime, actor health, version info |

Knowledge graph tools require `mcp.knowledge_graph.enabled = true` in config (default `false`).

## Patterns

- **Shared state**: same `Arc` pointers as pylon's `AppState` - zero duplication, zero serialization overhead.
- **Rate limiting**: per-session, two tiers (Cheap/Expensive), configurable via `mcp.rate_limit` config.
- **Path sanitization**: `sanitize::strip_paths()` removes server file paths before errors reach MCP clients.
- **Two transports**: streamable HTTP (mounted into pylon's Axum router at `/mcp`) and stdio (for CLI use).
- **Feature gated**: `mcp` feature in the binary crate; disabled by default.

## Common tasks

| Task | Where |
|------|-------|
| Add MCP tool | `src/tools/mod.rs` (new `#[tool]` method on DiaporeiaServer) + `src/tools/params.rs` (param struct) |
| Add MCP resource | `src/resources/` (new module) + `src/server.rs` (list/read handlers) |
| Modify rate limits | `src/rate_limit.rs` (Tier enum, RateLimiter) |
| Modify error mapping | `src/error.rs` (Error enum + From<Error> for rmcp::ErrorData) |

## Dependencies

Uses: koina, taxis, nous, organon, mneme, symbolon, rmcp, axum, serde_json, snafu, tracing
Used by: aletheia (binary, optional via `mcp` feature)
