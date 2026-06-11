# aletheia-memory-mcp

Standalone stdio MCP server exposing Aletheia's nous local knowledge store to external agents (Claude Code, Cursor, OpenHands, etc.) without requiring the full Aletheia runtime. This is the session-scoped Aletheia nous store, not kanon mnemosyne's durable corpus.

## Tools

### Read tools (always available)

- `nous_search` - BM25 text search across active facts in the Aletheia nous local knowledge store
- `nous_neighbors` - one-hop graph traversal from a fact's entities; neighbor rows include `src_id`, `dst_id`, `name`, `entity_type`, `relation`, and `weight`
- `nous_list_topics` - enumerate fact-type buckets with counts
- `nous_stats` - knowledge graph health metrics (fact count, schema version, last updated)

### Write tools (capability-token gated)

- `nous_annotate` - create an annotation on an existing fact and link it back to the target fact
- `nous_supersede` - mark one fact as superseded by another
- `nous_forget` - soft-delete a fact (mark as forgotten)

## Configuration

### Environment variables

- `ALETHEIA_ROOT` - instance root directory (default: `./instance`). The knowledge store is opened at `<root>/data/knowledge.fjall`.
- `ALETHEIA_MEMORY_MCP_STORE` - override the store path directly.
- `ALETHEIA_MEMORY_MCP_WRITE_TOKEN` - capability token for write tools. If unset, write tools are not registered.
- `RUST_LOG` - tracing filter (default: `info`). Logs go to stderr; stdout is JSON-RPC only.

## Write tool authentication

Write tools are protected by a **per-process capability token** passed via environment variable at server startup.

### How it works

1. **Server Startup**: The spawning process (aletheia daemon or operator) generates a random token and sets it in the child's environment as `ALETHEIA_MEMORY_MCP_WRITE_TOKEN`. If this variable is unset, write tools are not registered or listed by MCP discovery.
2. **Token Validation**: Each write tool call includes a `write_token` field in its input. The server compares it against the configured token using constant-time comparison (via `subtle::ConstantTimeEq`).
3. **Authorization**: If tokens match, the write proceeds. If they don't match, the call returns an "unauthorized" error.
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
    "session_id": "agent-uuid",
    "write_token": "..."  // must match ALETHEIA_MEMORY_MCP_WRITE_TOKEN
  }
}
```

## Security notes

- **Capability Token**: This is a shared secret, not user authentication. Any process with access to the server's environment can invoke writes if it knows the token.
- **No Encryption**: The token is passed in-process via environment variable and in-protocol via MCP calls. It's not encrypted on the wire; use this server only over local IPC or secure channels (SSH, TLS).
- **Constant-Time Comparison**: Token comparison uses `subtle::ConstantTimeEq` to prevent timing-based token leakage.

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
