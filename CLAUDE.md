# CLAUDE.md

## At a glance

Repo-level conventions for AI coding agents working on Aletheia. Key crates: aletheia, nous, pylon, mneme. Entry point: `crates/aletheia/src/main.rs`.

## Depth

Build the system you'd trust to run without you.

Project conventions for AI coding agents working on this codebase.

See also [AGENTS.md](AGENTS.md) for a cross-tool quick-reference (build commands, crate selection flowchart, common mistakes).

## Standards

Universal: [standards/STANDARDS.md](standards/STANDARDS.md)
Rust: [standards/RUST.md](standards/RUST.md)
Writing: [standards/WRITING.md](standards/WRITING.md)
Shell: [standards/SHELL.md](standards/SHELL.md)

## Structure

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): crate workspace, module map, dependency graph, trait boundaries.  
[docs/ARCHITECTURE-QUICK.md](docs/ARCHITECTURE-QUICK.md): one-page crate reference for cold-start.

### Config

- **Rust crates:** `instance.example/config/aletheia.toml` (TOML cascade: defaults → TOML → env vars)

## Commands

```bash
cargo build                            # Debug build
cargo build --release                  # Release (LTO, stripped)
cargo test --workspace                 # Default tests (~5,400)
cargo test --workspace --features test-core  # + storage/engine tests (~5,435)
cargo test --workspace --features test-full  # + ML embedding tests (~5,435)
cargo test -p aletheia-hermeneus       # Single crate
cargo clippy --workspace               # Lint (zero warnings)
```

Test tiers: [docs/test-tiers.md](docs/test-tiers.md)

## Key patterns

- **Errors:** `snafu` with `.context()` propagation and `Location` tracking
- **IDs:** Newtypes for all domain IDs (`AgentId`, `SessionId`, `NousId`)
- **Time:** `jiff` for time, `ulid` for IDs, `compact_str` for small strings
- **Async:** Tokio actor model (`NousActor` pattern)
- **Config:** TOML cascade in `taxis` (owned loader, no figment)
- **Lints:** `#[expect(lint, reason = "...")]` over `#[allow]`; every suppression justified
- **Visibility:** `pub(crate)` by default; `pub` only for cross-crate API surface
- **Naming:** Greek names per [standards/GNOMON.md](standards/GNOMON.md), registry at [docs/lexicon.md](docs/lexicon.md)
- **No barrel files**: import from the file that owns the symbol
- **Module imports flow downward**: higher layers depend on lower, never reverse

## Before submitting

1. `cargo test -p <affected-crate>` passes
2. `cargo clippy --workspace`: zero warnings
3. No `unwrap()` in library code
4. New errors use snafu with context
5. All lint suppressions use `#[expect]` with reason, not `#[allow]`

## Test data & instance boundary

- All test data MUST use synthetic identities (alice, bob, acme.corp, 192.168.1.100)
- NEVER use real personal information in test fixtures or example data
- Operator-specific config belongs in `instance/` (gitignored), not `shared/` or repo root
- `instance.example/` shows the expected structure for fresh clones
- CI PII scanner rejects commits with personal data patterns (`.github/pii-patterns.txt`)

## Where to add things

| Task | Location | Registration |
|------|----------|-------------|
| CLI subcommand | `crates/aletheia/src/commands/` | Add to clap derive in `main.rs` |
| API endpoint | `crates/pylon/src/handlers/` | Add route in `crates/pylon/src/router.rs` |
| Built-in tool | `crates/organon/src/builtins/` | Register in `register_all()` |
| Config section | `crates/taxis/src/config.rs` | Add field to `AletheiaConfig` struct |
| Error variant | `crates/{crate}/src/error.rs` | snafu derive on the crate's Error enum |
| Maintenance task | `crates/daemon/src/` | Register in runner |
| Bootstrap file | `crates/nous/src/bootstrap/` | Add to section list in assembler |
| Middleware | `crates/pylon/src/middleware/` | Add layer in `crates/pylon/src/server.rs` |

Per-crate details in each crate's `CLAUDE.md`.

## Adding tools

Tools live in `crates/organon/`. Each tool implements the `ToolExecutor` trait:

```rust
pub trait ToolExecutor: Send + Sync {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>>;
}
```

Register in `crates/organon/src/builtins/mod.rs` via `register_all()`.

## Scripts

| Script | Usage |
|--------|-------|
| `scripts/deploy.sh` | Deploy binary to local instance. `--build` to compile, `--restart` to restart service. No flags = full deploy. |
| `scripts/health-monitor.sh` | Health check: service status, API health, token expiry, LLM cost. `--notify` for Signal alerts. |

Systemd timer for health monitoring: `instance.example/services/aletheia-health.{service,timer}` (5-minute interval).

## Git

Conventional commits: `<type>(<scope>): <description>`. Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `ci`, `perf`. Present tense imperative, first line ≤72 chars. Scope is the crate name.

| Branch Type | Pattern | Example |
|-------------|---------|---------|
| Feature | `feat/<description>` | `feat/recall-pipeline` |
| Bug fix | `fix/<description>` | `fix/session-timeout` |
| Docs | `docs/<description>` | `docs/deployment-guide` |
| Refactor | `refactor/<description>` | `refactor/config-cascade` |
| Chore | `chore/<description>` | `chore/update-deps` |

Branch from `main`. Rebase before pushing. Always squash merge.
