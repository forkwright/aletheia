<!--
scope: aletheia repo conventions (cognition-server crates, recipes, organon tools, theatron UI)
defers_to: ~/menos-ops/CLAUDE.md for machine topology; ~/.claude/CLAUDE.md for operator principles (including "build the system you'd trust to run without you")
tightens: per-crate CLAUDE.md files under crates/*/ can narrow conventions within their blast radius
-->

# CLAUDE.md

## At a glance

Repo-level conventions for AI coding agents working on Aletheia. Key crates: aletheia, nous, pylon, mneme. Entry point: `crates/aletheia/src/main.rs`.

## Depth

See also [AGENTS.md](AGENTS.md) for a cross-tool quick-reference (build commands, crate selection flowchart, common mistakes).

## Standards

Universal: `~/dev/kanon/crates/basanos/standards/STANDARDS.md`
Rust: `~/dev/kanon/crates/basanos/standards/RUST.md`
Writing: `~/dev/kanon/crates/basanos/standards/WRITING.md`
Shell: `~/dev/kanon/crates/basanos/standards/SHELL.md`

## Structure

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): crate workspace, module map, dependency graph, trait boundaries.
[docs/ARCHITECTURE-QUICK.md](docs/ARCHITECTURE-QUICK.md): one-page crate reference for cold-start.

### Loading recipes

Task-specific context selection is defined in [`_llm/recipes.toml`](_llm/recipes.toml). Use it to decide which resolution levels to load:

| Task type | Recipe | What to load | Token budget |
|-----------|--------|--------------|-------------|
| Cold start / first interaction | `cold_start` | `L1-workspace.md` + `manifest.toml` + `CLAUDE.md` | ~5,700 |
| Single-crate edit | `edit_crate` | L2(crate) + L3(crate) + source(crate) | varies |
| Cross-crate refactor | `cross_crate_refactor` | `L1-workspace.md` + `manifest.toml` + L3(touched crates) | ~250K |
| New HTTP endpoint | `add_endpoint` | `architecture.toml` + `api.toml` + L3(pylon, nous) | ~65K |
| New built-in tool | `add_tool` | `architecture.toml` + L3(organon) + `builtins/` source | ~35K |
| Bug fix in known crate | `fix_bug` | `architecture.toml` + L3(crate) + source(crate) | ~45K |

The `nous` crate provides [`RecipeRegistry`](crates/nous/src/recipes.rs) for parsing and selecting recipes at bootstrap time. L2 summaries now include one-line recent-change notes; use them before reading source for post-2026-05-08 substrate orientation.

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

### Mutation testing

`cargo-mutants` mutates the source and re-runs the tests; a mutation that
passes all tests is a test that does not actually test the code it claims
to cover. Install once per machine, then run against the changed crate
(full workspace takes hours):

```bash
cargo install cargo-mutants
cargo mutants --package <crate> --baseline=skip   # mutate a whole crate
cargo mutants --baseline=skip --in-diff <branch>  # mutate only the diff vs <branch>
```

Baselines for critical paths live under `mutants-out/` (gitignored). Treat
any **missed** mutation as a test gap: either strengthen the existing
assertion or add a new test that catches the mutant. See `docs/RELEASING.md`
for the release-time substance-audit gate that calls this tool via
`kanon audit substance`.

## Key patterns

- **Errors:** `snafu` with `.context()` propagation and `Location` tracking
- **IDs:** Newtypes for all domain IDs (`AgentId`, `SessionId`, `NousId`)
- **Time:** `jiff` for time, `ulid` for IDs
- **Async:** Tokio actor model (`NousActor` pattern)
- **Config:** TOML cascade in `taxis` (owned loader, no figment)
- **Lints:** `#[expect(lint, reason = "...")]` over `#[allow]`; every suppression justified
- **Visibility:** `pub(crate)` by default; `pub` only for cross-crate API surface
- **Naming:** Greek names per `~/dev/kanon/crates/basanos/standards/GNOMON.md`, registry at [docs/lexicon.md](docs/lexicon.md)
- **No barrel files**: import from the file that owns the symbol
- **Module imports flow downward**: higher layers depend on lower, never reverse

## Before submitting

Install the in-tree hooks once per clone: `scripts/install-hooks.sh` (auto-run by `.envrc`/direnv). The `pre-push` hook runs CI-exact fmt + `_llm` freshness + clippy so they never first-fail in CI; deliberate bypass is `git push --no-verify`.

1. `cargo +1.94.0 fmt --all -- --check` clean — **fmt is a required CI check** and runs *first* in gate-attestation, so a fmt-only miss aborts the whole gate
2. `cargo test -p <affected-crate>` passes
3. `cargo clippy --workspace`: zero warnings (CI-exact: `--all-targets --exclude theatron --exclude skene --exclude koilon -- -D warnings`)
4. `_llm` index fresh on crate changes: `uv run scripts/llm-extract-l3.py && git add _llm/`
5. No `unwrap()` in library code
6. New errors use snafu with context
7. All lint suppressions use `#[expect]` with reason, not `#[allow]`

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
| Config section | `crates/taxis/src/config/behavior/` or `crates/taxis/src/config/agents.rs` | Add field to `AletheiaConfig`/defaults and registry metadata |
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
