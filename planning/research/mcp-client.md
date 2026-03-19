# R712: MCP Client Support

## Question

Aletheia has MCP server support (diaporeia crate) but no MCP client capability. What is required for aletheia agents to connect to external MCP servers, discover their tools and resources, and invoke them alongside native organon tools? What are the protocol requirements, transport options, security considerations, and integration design?

## Findings

### 1. existing MCP server audit (diaporeia)

**Crate**: `aletheia-diaporeia` at `crates/diaporeia/`

**SDK**: `rmcp = "=1.2.0"` (exact pin, pre-1.0 with rapid release cadence). Features enabled: `server`, `macros`, `transport-io`, `transport-streamable-http-server`.

**Protocol version**: MCP 2025-11-25 (implied by rmcp 1.2.0).

**Transport**: Streamable HTTP via Axum router mounted at `/mcp`. Uses `StreamableHttpService` with `LocalSessionManager`. A secondary `serve_stdio()` function exists but is `#[expect(dead_code)]` pending an `aletheia mcp` subcommand.

**Tools exposed**: 10 tools via `#[tool_router]` macro, organized into session management (4), agent discovery (3), and knowledge/system (3). Two rate-limit tiers: expensive (60 req/min for mutations) and cheap (300 req/min for reads).

**Resources exposed**: 5 nous workspace file templates (`aletheia://nous/{nous_id}/soul|identity|memory|goals|tools`) plus 1 config resource (`aletheia://config`).

**Architecture**: `DiaporeiaServer` implements `rmcp::handler::server::ServerHandler`. Shares `Arc` pointers with pylon's `AppState` for zero-copy access to `NousManager`, `ToolRegistry`, `SessionStore`, `Oikos`, and `AletheiaConfig`. Feature-gated behind `mcp` (disabled by default).

**Error handling**: 6 error variants mapped to JSON-RPC codes. Path sanitization strips server-side paths from error messages before they reach clients.

### 2. MCP protocol requirements for client role

The MCP spec (2025-11-25) defines three roles: host (aletheia), client (connector per server), and server (external tool provider). Each client maintains a 1:1 stateful session with one server.

#### 2.1 lifecycle

Three-phase handshake:
1. Client sends `initialize` with protocol version, capabilities, and client info.
2. Server responds with its capabilities, server info, and optional instructions.
3. Client sends `notifications/initialized`. Normal operations begin.

Version negotiation: client proposes latest version it supports; server echoes it or responds with an older version. If incompatible, client disconnects.

Shutdown: client sends `notifications/cancelled` for in-flight requests, then closes transport.

#### 2.2 client-to-Server methods

| Method | Purpose | Paginated |
|--------|---------|-----------|
| `tools/list` | Discover available tools | Yes |
| `tools/call` | Invoke a tool | No |
| `resources/list` | List resources | Yes |
| `resources/read` | Read resource content | No |
| `resources/templates/list` | List URI templates | Yes |
| `resources/subscribe` | Subscribe to changes | No |
| `prompts/list` | List prompt templates | Yes |
| `prompts/get` | Get expanded prompt | No |
| `completion/complete` | Argument autocompletion | No |
| `logging/setLevel` | Set server log verbosity | No |
| `ping` | Health check | No |

#### 2.3 server-to-Client methods (optional client features)

| Method | Purpose | Phase |
|--------|---------|-------|
| `sampling/createMessage` | Server requests LLM completion | 4 |
| `roots/list` | Server asks for filesystem roots | 1 |
| `elicitation/create` | Server requests user input | 4 |

#### 2.4 notifications

Client receives: `notifications/tools/list_changed`, `notifications/resources/list_changed`, `notifications/resources/updated`, `notifications/prompts/list_changed`, `notifications/progress`, `notifications/message` (logging).

Client sends: `notifications/initialized`, `notifications/cancelled`, `notifications/roots/list_changed`.

### 3. transport options

#### 3.1 stdio

Client launches server as a subprocess. JSON-RPC messages on stdin/stdout, newline-delimited. Server logs to stderr. Shutdown: close stdin, wait, SIGTERM, SIGKILL. Simplest transport, recommended by spec for local servers.

#### 3.2 streamable HTTP

Single endpoint (e.g., `https://example.com/mcp`). POST for client requests (returns JSON or SSE stream). Optional GET for server-initiated SSE stream. Session management via `MCP-Session-Id` header. Resumable streams via `Last-Event-ID`. Protocol version header (`MCP-Protocol-Version`) required on all requests.

#### 3.3 HTTP+SSE (legacy, 2024-11-05)

Separate GET endpoint (returns SSE with `endpoint` event) and POST endpoint. Backwards-compatibility fallback: try POST initialize first, fall back to GET if 400/404/405.

#### 3.4 no webSocket

The spec does not define a WebSocket transport despite community discussion.

### 4. authentication

OAuth 2.1 for HTTP transports (optional). Discovery via Protected Resource Metadata (RFC 9728). PKCE required (S256). `resource` parameter (RFC 8707) required. Three client registration approaches: pre-registered, client ID metadata documents, dynamic registration (RFC 7591). Tokens via `Authorization: Bearer` on every request.

stdio transport: credentials from environment, not OAuth.

### 5. rmcp client support

rmcp 1.2.0 has full client support via the `client` feature flag (not enabled in aletheia).

**Key client features available:**
- `client`: Core client protocol, `Peer<RoleClient>` API
- `transport-child-process`: Subprocess management for stdio
- `transport-streamable-http-client-reqwest`: HTTP client with reqwest (aletheia already uses reqwest)
- `auth`: OAuth 2.0 support

**Client API surface** (from `Peer<RoleClient>`):
- `list_tools()`, `list_all_tools()` (pagination-aware), `call_tool()`
- `list_resources()`, `read_resource()`, `subscribe()`
- `list_prompts()`, `get_prompt()`
- `complete()`

**Initialization flow:**
```rust
let transport = TokioChildProcess::new(command)?;
let running = serve_client(client_handler, transport).await?;
let peer = running.peer();
let tools = peer.list_all_tools().await?;
```

The SDK abstracts JSON-RPC framing, lifecycle management, and transport details. No need to implement the protocol layer from scratch.

### 6. organon tool system integration points

**Tool trait**: `ToolExecutor` (object-safe async trait in `crates/organon/src/registry.rs`). Single method: `execute(&self, input: &ToolInput, ctx: &ToolContext) -> Result<ToolResult>`.

**Registry**: `ToolRegistry` holds `IndexMap<ToolName, RegisteredTool>`. Methods: `register()`, `execute()`, `definitions()`, `to_hermeneus_tools_filtered()`. External tools would register identically to builtins.

**Activation model**: Two tiers. `auto_activate = true` tools always appear in LLM context. `auto_activate = false` tools are "lazy" and activated per-session via the `enable_tool` meta-tool. MCP tools are natural candidates for lazy activation (they are remote and add context-window cost).

**Tool dispatch**: `crates/nous/src/execute/dispatch.rs` extracts tool calls from LLM responses, calls `tools.execute()`, and collects results. Transport-agnostic: dispatch does not know whether a tool is local or remote.

**Tool services**: `ToolServices` (`crates/organon/src/types.rs`) provides dependency injection. An MCP client connection manager could be added here.

**Domain packs**: `thesauros` already registers external tools (from domain packs) using the same `ToolRegistry`. Precedent exists for non-builtin tool registration.

### 7. configuration system

**Framework**: figment (defaults, TOML file, environment variables). All config types in `crates/taxis/src/config.rs`.

**Existing MCP config**: `McpConfig` with `rate_limit` sub-struct (for server rate limiting). This is the natural extension point for client configuration.

**Patterns to follow**: `ChannelsConfig` uses `HashMap<String, AccountConfig>` for named external services. `packs` uses array of paths. Either pattern works for MCP server registration.

**Validation**: `crates/taxis/src/validate.rs` has per-section validators. MCP is not validated (falls to unknown-section handler). Any extension needs a validator.

---

## Design

### Configuration

Extend the existing `[mcp]` section in `aletheia.toml`:

```toml
[mcp]
# Existing server rate limiting
[mcp.rate_limit]
enabled = true
message_requests_per_minute = 60
read_requests_per_minute = 300

# New: external MCP server connections
[[mcp.servers]]
name = "filesystem"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/docs"]
enabled = true
trust = "sandboxed"          # "sandboxed" | "trusted" | "blocked"
timeout_seconds = 30
env = { HOME = "/home/user" }

[[mcp.servers]]
name = "github"
transport = "http"
url = "https://mcp.github.com/v1"
enabled = true
trust = "sandboxed"
timeout_seconds = 60
# OAuth config (optional)
auth = { type = "oauth", client_id = "abc123" }

[[mcp.servers]]
name = "local-db"
transport = "stdio"
command = "/usr/local/bin/db-mcp-server"
args = ["--readonly"]
enabled = false
trust = "trusted"            # Operator explicitly trusts this server
timeout_seconds = 10
```

**Config types** (new structs in `taxis::config`):

```
McpServerConfig {
    name: String,             // Unique identifier, used as namespace prefix
    transport: McpTransport,  // Stdio { command, args, env } | Http { url, auth }
    enabled: bool,
    trust: TrustLevel,        // Sandboxed | Trusted | Blocked
    timeout_seconds: u32,
    auto_activate_tools: Vec<String>,  // Tools to auto-activate (empty = all lazy)
}
```

### Tool namespace

External MCP tools are namespaced as `mcp_{server}_{tool}` in the organon registry.

Example: server named "filesystem" exposing tool "read_file" becomes `mcp_filesystem_read_file`.

This prevents collisions with native organon tools and makes the tool's origin visible in LLM context and logs. The `enable_tool` meta-tool's catalog shows MCP tools grouped by server.

### Tool integration architecture

```
                    ToolRegistry (organon)
                    /                    \
           Native tools              MCP proxy tools
           (builtins,                (McpToolExecutor)
            domain packs)                  |
                                    McpClientManager
                                    /              \
                              McpClient          McpClient
                              (stdio)            (http)
                                |                   |
                           subprocess          reqwest + SSE
                                |                   |
                          MCP Server A         MCP Server B
```

**McpClientManager**: Owns all MCP client connections. Stored in `ToolServices` or as a standalone `Arc`. Responsible for:
- Spawning/connecting clients at startup based on config
- Reconnecting on failure (with backoff)
- Health checking via `ping`
- Providing `Peer` references to `McpToolExecutor` instances

**McpToolExecutor**: Implements `ToolExecutor`. One instance per external tool. Holds a reference to the appropriate `McpClient`. On `execute()`:
1. Convert `ToolInput.arguments` to `CallToolRequestParams`
2. Call `peer.call_tool()` with timeout
3. Convert `CallToolResult` to `ToolResult`
4. Map `isError: true` to `ToolResult { is_error: true, .. }`
5. Map transport/protocol errors to organon error types

**Registration flow** (at startup):
1. Parse `[[mcp.servers]]` from config
2. For each enabled server, create transport and initialize client
3. Call `peer.list_all_tools()` to discover tools
4. For each tool, create `McpToolExecutor` and register in `ToolRegistry` with namespaced name
5. Set `auto_activate = false` by default (lazy loading)
6. Subscribe to `notifications/tools/list_changed` for dynamic updates

### Resource access

MCP resources from external servers are not directly exposed as organon tools. Instead:

1. A single `mcp_resources` tool (or per-server `mcp_{server}_resources`) lets agents list and read resources on demand.
2. Resource content is returned as tool output text.
3. Resource subscriptions are managed by `McpClientManager` and surfaced as notifications to the agent session.

This avoids polluting the tool registry with resource URIs and keeps the interface consistent with how agents interact with tools.

### Crate placement

Two options:

**Option A: Extend diaporeia** with a `client` module alongside the existing `server` module. Reuses the crate's MCP domain and the rmcp dependency. Add `client` feature flag to rmcp.

**Option B: New crate** `diaporeia-client` (or a gnomon-named crate). Cleaner separation but adds a crate to the workspace.

**Recommendation**: Option A. Diaporeia means "passage through" and already owns the MCP boundary. The client is the other direction of passage. Feature-gate the client (`mcp-client` feature) independently from the server (`mcp` feature). The rmcp dependency already exists; adding the `client` feature is a Cargo.toml change.

### Error handling

New error variants in `diaporeia::error`:

```
McpClientError {
    Connect { server: String, source: ... },
    Initialize { server: String, source: ... },
    ToolCall { server: String, tool: String, source: ... },
    ResourceRead { server: String, uri: String, source: ... },
    Timeout { server: String, operation: String },
    ServerDisconnected { server: String },
}
```

These map to organon's `ExecutionFailed` at the tool dispatch boundary. The agent sees a tool error with context about which MCP server failed.

---

## Security model

### Trust levels

Three trust levels per configured server:

| Level | Tool calls | Data exposure | Sampling | Confirmation |
|-------|-----------|---------------|----------|--------------|
| `blocked` | Denied | None | Denied | N/A |
| `sandboxed` | Allowed with restrictions | Tool arguments only, no session context | Denied | Required for destructive annotations |
| `trusted` | Allowed | Tool arguments, may include context | Allowed (phase 4) | Optional |

Default: `sandboxed`. Operator must explicitly set `trusted`.

### Data flow control

**Sandboxed servers**:
- Receive only the tool call arguments (name + JSON params) that the LLM explicitly constructs.
- Do not receive session history, system prompts, other tool results, or agent identity.
- Server `instructions` from initialization are included in the agent's context but clearly marked as external.

**Trusted servers**:
- Same as sandboxed, plus the client may expose `roots` and respond to `sampling` requests (phase 4).

**All servers**:
- Tool results from external servers are marked with their origin in tracing spans.
- Path sanitization (already in diaporeia for server mode) applies to client-received error messages too.
- Secret values from `symbolon` are never passed as tool arguments. The tool dispatch layer checks arguments against a redaction list.

### Subprocess isolation (stdio)

- Subprocess inherits only explicitly configured `env` variables, not the full parent environment.
- Working directory set to a temporary directory, not the aletheia instance root.
- On Linux, consider landlock/seccomp for subprocess sandboxing (aletheia already has landlock support in organon for shell execution).

### Network isolation (HTTP)

- TLS required for non-localhost URLs (`rustls`, consistent with aletheia's TLS policy).
- Connection timeout + per-request timeout from config.
- No automatic credential forwarding between servers (token audience binding per spec).

### Tool annotation trust

Per the MCP spec, tool annotations (like `readOnlyHint`, `destructiveHint`) are untrusted unless the server is `trusted`. For `sandboxed` servers, ignore annotations and apply conservative defaults (assume potentially destructive).

---

## Effort estimate

### Phase 1: stdio + tool discovery/Invocation (3-4 weeks)

- Extend `McpConfig` with `servers` array and `McpServerConfig` types
- Add config validation in taxis
- Enable rmcp `client` + `transport-child-process` features
- Implement `McpClientManager` (subprocess lifecycle, reconnection, health)
- Implement `McpToolExecutor` (tool call proxy with timeout and error mapping)
- Tool registration in organon registry at startup
- Namespace prefix scheme
- Lazy activation integration with `enable_tool`
- Trust level enforcement (sandboxed by default)
- Tests: mock MCP server via stdio for integration tests

### Phase 2: resources, prompts, and dynamic updates (2 weeks)

- `mcp_resources` tool for resource listing and reading
- Prompt listing and expansion
- Handle `notifications/tools/list_changed` (re-register tools dynamically)
- Resource subscriptions
- Completion support

### Phase 3: streamable HTTP transport (3-4 weeks)

- Enable rmcp `transport-streamable-http-client-reqwest` feature
- Session management (`MCP-Session-Id`)
- Resumable SSE streams
- OAuth 2.1 flow (discovery, PKCE, token management, refresh)
- Legacy HTTP+SSE backwards compatibility
- TLS enforcement for remote servers

### Phase 4: advanced client features (2-3 weeks)

- Sampling support (route server LLM requests through hermeneus)
- Elicitation support (form + URL modes, requires TUI/UI integration)
- Task-augmented requests for long-running operations
- Progress tracking and cancellation forwarding

**Total: 10-13 weeks across all phases.** Phase 1 alone delivers the core value proposition (local MCP tool access).

---

## Gotchas

1. **rmcp version pin**: rmcp is pinned at `=1.2.0` because of rapid pre-1.0 churn (5 minor releases in 6 weeks per TECHNOLOGY.md). Enabling client features at the same pinned version avoids SDK version conflicts. If a newer rmcp version has breaking changes, both server and client must upgrade together.

2. **Session statefulness**: Each client-server pair is a stateful session. Cannot share sessions across agents or reuse after disconnect. `McpClientManager` must handle per-agent or shared-pool connection strategies. Shared pool is simpler (tools are stateless from the agent's perspective) but some servers may track per-session state.

3. **Tool name collisions**: The `mcp_{server}_{tool}` namespace prefix is critical. Without it, an external server could shadow native tools like `read` or `exec`. The namespace also appears in LLM tool definitions, so tool descriptions must be clear about the tool's origin.

4. **Context window cost**: Each registered MCP tool adds to the LLM context window (name, description, input schema). With multiple servers exposing many tools, this can consume significant token budget. Lazy activation via `enable_tool` mitigates this: MCP tools start inactive and agents enable them on demand. The `auto_activate_tools` config option allows operators to pre-activate high-value tools.

5. **Subprocess lifecycle**: stdio servers are child processes. If aletheia crashes, orphaned subprocesses persist. Use process groups and a cleanup-on-startup sweep. The existing worktree cleanup pattern in dispatch is analogous.

6. **OAuth complexity**: Full OAuth 2.1 is a significant implementation. The discovery alone (Protected Resource Metadata -> authorization server metadata -> well-known endpoints) involves multiple HTTP round trips and caching. Defer to phase 3 and use rmcp's `auth` feature which handles some of this.

7. **No WebSocket**: Despite expectations, the spec has no WebSocket transport. Only stdio and Streamable HTTP. This simplifies the transport matrix but limits some deployment patterns.

8. **Dynamic tool registration**: When a server sends `notifications/tools/list_changed`, the client must re-fetch tools and update the registry. This means `ToolRegistry` needs a mechanism for removing/replacing tools at runtime. Currently registration is startup-only. This requires either a new `unregister()`/`replace()` method or a registry rebuild.

9. **JSON Schema dialect**: MCP uses JSON Schema 2020-12 for tool input/output schemas. The `schemars` crate (already in diaporeia for server-side schema generation) and `jsonschema` crate support this, but verify compatibility with the exact dialect servers produce.

10. **Timeout propagation**: MCP has no built-in timeout mechanism. The client must enforce timeouts at the transport level. rmcp's `call_tool()` does not take a timeout parameter; wrap in `tokio::time::timeout()`.

11. **`McpConfig` validation gap**: The current `validate_section()` in `taxis/src/validate.rs` does not validate the `mcp` section (it falls to the unknown-section catch-all). Adding `servers` config requires adding an `"mcp"` arm to the match with validation for transport types, URLs, timeouts, and trust levels.

---

## References

- MCP Specification 2025-11-25: https://modelcontextprotocol.io/specification/2025-11-25
- MCP Architecture: https://modelcontextprotocol.io/specification/2025-11-25/architecture
- MCP Lifecycle: https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle
- MCP Transports: https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- MCP Authorization: https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization
- MCP Tools: https://modelcontextprotocol.io/specification/2025-11-25/server/tools
- MCP Resources: https://modelcontextprotocol.io/specification/2025-11-25/server/resources
- MCP Sampling: https://modelcontextprotocol.io/specification/2025-11-25/client/sampling
- rmcp crate (Rust MCP SDK): version 1.2.0, features: client, server, transport-child-process, transport-streamable-http-client-reqwest, auth
- Aletheia diaporeia crate: `crates/diaporeia/` (server implementation)
- Aletheia organon crate: `crates/organon/` (tool registry and executor trait)
- Aletheia taxis crate: `crates/taxis/` (configuration system)
- Aletheia TECHNOLOGY.md: rmcp pin rationale (5 minor releases in 6 weeks)
