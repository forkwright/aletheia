# AGENTS.md — Aletheia

Cross-tool guide for AI coding agents (Claude Code, Cursor, Windsurf, Copilot, etc.).

## Build / Test / Lint

```bash
cargo check -p <crate>                 # fast compile check
cargo test -p <crate>                  # single crate tests
cargo clippy --workspace               # lint (zero warnings)
cargo test --workspace                 # full suite
```

Desktop crate (`proskenion`) excluded from workspace — build standalone:
`cargo check --manifest-path crates/theatron/proskenion/Cargo.toml`

## Key Patterns

- **Errors:** `snafu` with `.context()`. No `unwrap()` in library code.
- **IDs:** Newtypes (`AgentId`, `SessionId`, `NousId`). `ulid` for generation.
- **Time:** `jiff`. **Strings:** `compact_str` for small strings.
- **Async:** Tokio actor model (`NousActor` pattern).
- **Config:** figment TOML cascade in `taxis`.
- **Lints:** `#[expect(lint, reason = "...")]` not `#[allow]`.
- **Visibility:** `pub(crate)` default. `pub` only for cross-crate API.
- **Naming:** Greek names per `docs/lexicon.md`. No barrel files.
- **Imports:** Flow downward only (leaf → low → mid → high → top).
- **Commits:** `type(scope): description`. Scope = crate name.
- **Test data:** Synthetic identities only (alice, bob, acme.corp).

## Where to Add Things

| Task | Location | Registration |
|------|----------|-------------|
| CLI subcommand | `crates/aletheia/src/commands/` | Add to clap derive in `main.rs` |
| API endpoint | `crates/pylon/src/handlers/` | Route in `crates/pylon/src/router.rs` |
| Built-in tool | `crates/organon/src/builtins/` | `register_all()` |
| Config section | `crates/taxis/src/config.rs` | Field on `AletheiaConfig` |
| Error variant | `crates/{crate}/src/error.rs` | snafu derive on Error enum |
| Maintenance task | `crates/daemon/src/` | Register in runner |
| Bootstrap file | `crates/nous/src/bootstrap/` | Section list in assembler |
| Middleware | `crates/pylon/src/middleware/` | Layer in `crates/pylon/src/server.rs` |

Crate architecture and dependency graph: `docs/ARCHITECTURE.md`.

## Gate Trailer

All PRs need `Gate-Passed: kanon 0.1.0` in a commit body.
Desktop PRs: `Gate-Passed: kanon 0.1.0 desktop-only (excluded from workspace build)`.
