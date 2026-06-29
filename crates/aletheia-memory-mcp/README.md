# aletheia-memory-mcp

Standalone stdio MCP server exposing Aletheia's nous local knowledge store to external agents (Claude Code, Cursor, OpenHands, etc.) without requiring the full Aletheia runtime. This is the session-scoped Aletheia nous store, not kanon mnemosyne's durable corpus.

## Tools

### Read tools (always available)

- `nous_search` - BM25 text search across active facts in the Aletheia nous local knowledge store
- `nous_neighbors` - one-hop graph traversal from a fact's entities; neighbor rows include `src_id`, `dst_id`, `name`, `entity_type`, `relation`, and `weight`
- `nous_list_topics` - enumerate fact-type buckets with counts
- `nous_stats` - knowledge graph health metrics (fact count, schema version, opaque store id, backend, readiness, last updated)

### Write tools (capability-token gated)

- `nous_annotate` - create an annotation on an existing fact and link it back to the target fact
- `nous_supersede` - mark one fact as superseded by another
- `nous_forget` - soft-delete a fact (mark as forgotten)

## Configuration

### Environment variables

- `ALETHEIA_ROOT` - instance root directory (default: `./instance`). The knowledge store is opened at `<root>/data/knowledge.fjall/shared` (the shared episteme cohort).
- `ALETHEIA_MEMORY_MCP_STORE` - override the store path directly; use this to target a different cohort, e.g. `<root>/data/knowledge.fjall/<cohort>`. `nous_stats` returns an opaque fingerprint for this path by default.
- `ALETHEIA_MEMORY_MCP_NOUS_ID` - bind read tools to a single caller identity. Read tools fail closed when this is unset.
- `ALETHEIA_MEMORY_MCP_WRITE_TOKEN` - capability token for write tools. If unset, write tools are not registered.
- `ALETHEIA_MEMORY_MCP_ADMIN_DIAGNOSTICS` - set to `1`, `true`, `yes`, or `admin` to permit full store-path diagnostics. This only takes effect when `ALETHEIA_MEMORY_MCP_WRITE_TOKEN` is also configured, and `nous_stats` still requires `include_store_path: true`.
- `RUST_LOG` - tracing filter (default: `info`). Logs go to stderr; stdout is JSON-RPC only.

## Write tool authentication

Write tools are exposed only when a **per-process capability token** is passed via environment variable at server startup.

### How it works

1. **Server Startup**: The spawning process (aletheia daemon or operator) generates a random token and sets it in the child's environment as `ALETHEIA_MEMORY_MCP_WRITE_TOKEN`. If this variable is unset, write tools are not registered or listed by MCP discovery.
2. **Route Registration**: Write tools are registered only when the token passes startup validation. The token is not part of any model-visible tool schema.
3. **Authorization**: If the token is missing or invalid at startup, write tools stay unavailable and direct calls fail closed.
4. **Audit Logging**: Successful writes are logged at INFO level to stderr with the tool name and affected fact IDs.

### Token generation

Operators should generate tokens cryptographically:

```bash
openssl rand -hex 32
```

This produces a 64-character hexadecimal string suitable for use as `ALETHEIA_MEMORY_MCP_WRITE_TOKEN`.

### Example: spawning with write access

```bash
export ALETHEIA_MEMORY_MCP_WRITE_TOKEN=$(openssl rand -hex 32)
aletheia-memory-mcp  # server inherits the token
```

### Example: MCP client call

```json
{
  "name": "nous_annotate",
  "arguments": {
    "fact_id": "f-abc-123",
    "content": "Verified against external source X",
    "nous_id": "agent-uuid",
    "source_session_id": "session-uuid"
  }
}
```

## Security notes

- **Capability Token**: This is startup capability configuration, not per-client authentication. Any MCP client connected to a server process with write tools registered can invoke writes.
- **No Encryption**: The server communicates over the configured MCP transport without adding encryption; use this server only over local IPC or secure channels (SSH, TLS).
- **Path Redaction**: `nous_stats` returns an opaque store id by default. Full local paths require admin diagnostics at startup plus an explicit `include_store_path` request.

## Admission control

Write tools respect the knowledge store's `DefaultAdmissionPolicy` before persisting facts. If a policy rejects a write, the call fails with an admission error.

## Exit codes

- `0` - clean shutdown (peer closed connection)
- `1` - startup or transport error (details on stderr)

## Development

Run tests:

```bash
cargo nextest run -p aletheia-memory-mcp
cargo test -p aletheia-memory-mcp --doc  # doctest examples
```

Format and lint:

```bash
cargo fmt -p aletheia-memory-mcp
cargo clippy -p aletheia-memory-mcp --all-targets -- -D warnings
```

Full gate (simulates CI):

```bash
kanon gate --full  # from a public kanon checkout
```
