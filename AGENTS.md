# AGENTS.md — Aletheia

Condensed guide for AI coding agents. For full conventions, see `CLAUDE.md`.

## Build

```bash
cargo check -p <crate>                 # fast compile check
cargo test -p <crate>                  # single crate tests
cargo clippy --workspace               # lint (zero warnings)
cargo test --workspace                 # all tests (~5,400)
```

Desktop crate (`proskenion`) is excluded from workspace. Build standalone:
```bash
cargo check --manifest-path crates/theatron/proskenion/Cargo.toml
```

## Key patterns

- **Errors:** `snafu` with `.context()` propagation. No `unwrap()` in library code.
- **IDs:** Newtypes for all domain IDs (`AgentId`, `SessionId`, `NousId`).
- **Time:** `jiff` for time, `ulid` for IDs, `compact_str` for small strings.
- **Async:** Tokio actor model (`NousActor` pattern).
- **Config:** figment TOML cascade in `taxis`.
- **Lints:** `#[expect(lint, reason = "...")]` not `#[allow]`.
- **Visibility:** `pub(crate)` default. `pub` only for cross-crate API.
- **Naming:** Greek names per `docs/lexicon.md`.
- **Imports:** Module imports flow downward only (leaf → low → mid → high → top).
- **Commits:** `type(scope): description`. Scope is crate name.

## Where to add things

See `CLAUDE.md` for the full "Where to add things" table (CLI commands, API endpoints, built-in tools, config sections, error variants, maintenance tasks, bootstrap files, middleware).

Crate architecture and dependency graph: `docs/ARCHITECTURE.md`.

## Gate trailer

All PRs need `Gate-Passed: kanon 0.1.0` in a commit body.
Desktop PRs: `Gate-Passed: kanon 0.1.0 desktop-only (excluded from workspace build)`.
