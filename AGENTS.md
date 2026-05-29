# AGENTS.md - Aletheia

Cross-tool guide for AI coding agents (Claude Code, Cursor, Windsurf, Copilot, etc.).

## Build / Test / Lint

```bash
cargo check -p <crate>                 # fast compile check
cargo test -p <crate>                  # single crate tests
cargo clippy --workspace               # lint (zero warnings)
cargo test --workspace                 # full suite
```

Desktop crate (`proskenion`) excluded from workspace - build standalone:
`cargo check --manifest-path crates/theatron/proskenion/Cargo.toml`

## Key Patterns

- **Errors:** `snafu` with `.context()`. No `unwrap()` in library code.
- **IDs:** Newtypes (`AgentId`, `SessionId`, `NousId`). `ulid` for generation.
- **Time:** `jiff`. **Strings:** `compact_str` for small strings.
- **Async:** Tokio actor model (`NousActor` pattern).
- **Config:** TOML cascade in `taxis` (owned loader, no figment).
- **Lints:** `#[expect(lint, reason = "...")]` not `#[allow]`.
- **Visibility:** `pub(crate)` default. `pub` only for cross-crate API.
- **Naming:** Greek names per `docs/lexicon.md`. No barrel files.
- **Imports:** Flow downward only (leaf → low → mid → high → top).
- **Commits:** `type(scope): description`. Scope = crate name.
- **Test data:** Synthetic identities only (alice, bob, acme.corp).

## Where to add things

| Task | Location | Registration |
|------|----------|-------------|
| HTTP endpoint | `crates/pylon/src/handlers/` | Route in `crates/pylon/src/router.rs` + OpenAPI spec |
| Tool | `crates/organon/src/builtins/` | `register_all()` |
| Config field | `crates/taxis/src/config/behavior/{domain}.rs` | Field on `AletheiaConfig` |
| Knowledge type | `crates/eidos/src/knowledge/` | Add to shared knowledge types |
| Pipeline stage | `crates/nous/src/pipeline/` | Wire into turn pipeline |
| Maintenance task | `crates/daemon/src/maintenance/` | Register in runner |
| CLI subcommand | `crates/aletheia/src/commands/` | Add to clap derive in `main.rs` |
| Provider | `crates/hermeneus/src/{provider}/` | Register in provider factory |
| Datalog rule | `crates/krites` | Add rule to embedded Datalog engine |
| Dispatch stage | `crates/energeia` | Wire into dispatch orchestration |
| Error variant | `crates/{crate}/src/error.rs` | snafu derive on Error enum |
| Bootstrap file | `crates/nous/src/bootstrap/` | Section list in assembler |
| Middleware | `crates/pylon/src/middleware/` | Layer in `crates/pylon/src/server.rs` |

Crate architecture and dependency graph: `docs/ARCHITECTURE.md`.

## Common Mistakes

- **Don't add a new crate if a function in an existing crate would do.** See #3501.
- **Don't add `rusqlite`-dependent code** - we use `fjall`.
- **Don't add ad-hoc HTTP clients** - use `hermeneus` or `organon/http_client`.
- **Don't write `_ = result`** - handle the result or document why we intentionally drop it.

## _llm/ Freshness

If your PR changes Rust source files, `Cargo.toml`, or `scripts/llm-extract-l3.py`,
regenerate the L3 API index before pushing:

```bash
uv run scripts/llm-extract-l3.py
git add _llm/ && git commit -m "chore(_llm): regenerate L3 API index"
```

The `_llm freshness` CI check fails the PR if the index is stale.

## Gate Trailer

All PRs need `Gate-Passed: kanon 0.1.0` in a commit body.
Desktop PRs: `Gate-Passed: kanon 0.1.0 desktop-only (excluded from workspace build)`.

## Optional: LSP-powered navigation

External MCP servers can give an agent IDE-level navigation across the workspace. See [docs/MCP-SERVERS.md](docs/MCP-SERVERS.md) - `scripts/serena-mcp.sh register` wires up Serena (rust-analyzer via MCP) for `find_symbol`, `find_referencing_symbols`, and `rename_symbol` across crate boundaries.
