# CLAUDE.md

Project conventions for AI coding agents working on this codebase.

## Standards

Full reference: [docs/STANDARDS.md](docs/STANDARDS.md).

@.claude/rules/rust.md
@.claude/rules/python.md

## Structure

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) - crate workspace, module map, dependency graph, trait boundaries.

### Config

- **Rust crates:** `instance/config/aletheia.yaml` - figment cascade (defaults → YAML → env vars)
- **Standards:** [docs/STANDARDS.md](docs/STANDARDS.md) - coding standards reference

## Commands

```bash
cargo build                            # Debug build
cargo build --release                  # Release (LTO, stripped)
cargo test --workspace                 # All tests
cargo test -p aletheia-hermeneus       # Single crate
cargo clippy --workspace               # Lint (zero warnings)
```

## Key Patterns

- **Errors:** `snafu` with `.context()` propagation and `Location` tracking
- **IDs:** Newtypes for all domain IDs (`AgentId`, `SessionId`, `NousId`)
- **Time:** `jiff` for time, `ulid` for IDs, `compact_str` for small strings
- **Async:** Tokio actor model (`NousActor` pattern)
- **Config:** figment YAML cascade in `taxis`
- **Lints:** `#[expect(lint, reason = "...")]` over `#[allow]` - every suppression justified
- **Visibility:** `pub(crate)` by default - `pub` only for cross-crate API surface
- **Naming:** Greek names per the gnomon convention ([docs/gnomon.md](docs/gnomon.md))
- **No barrel files** - import from the file that owns the symbol
- **Module imports flow downward** - higher layers depend on lower, never reverse

## Before Submitting

1. `cargo test -p <affected-crate>` passes
2. `cargo clippy --workspace` - zero warnings
3. No `unwrap()` in library code
4. New errors use snafu with context
5. All lint suppressions use `#[expect]` with reason, not `#[allow]`

## Test Data & Instance Boundary

- All test data MUST use synthetic identities (alice, bob, acme.corp, 192.168.1.100)
- NEVER use real personal information in test fixtures or example data
- Operator-specific config belongs in `instance/` (gitignored), not `shared/` or repo root
- `instance.example/` shows the expected structure for fresh clones
- CI PII scanner rejects commits with personal data patterns (`.github/pii-patterns.txt`)

## Adding Tools

Tools live in `crates/organon/`. Each tool implements the `ToolExecutor` trait:

```rust
pub trait ToolExecutor: Send + Sync {
    fn definition(&self) -> &ToolDefinition;
    fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolContext,
    ) -> Result<String, ToolError>;
}
```

Register in `crates/organon/src/builtins/mod.rs` via `register_all()`.

## CLI

```text
aletheia                            # start server (default)
aletheia health [--url URL]         # check if server is running
aletheia status [--url URL]         # system status dashboard
aletheia backup [--list] [--prune --keep N] [--export-json]
aletheia maintenance status         # show maintenance task status
aletheia maintenance run <task>     # run: trace-rotation, drift-detection, db-monitor, all
aletheia tls generate [--output-dir PATH] [--days N] [--san NAMES...]
```

## API Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/health` | Health check |
| GET | `/api/docs/openapi.json` | OpenAPI spec |
| GET | `/metrics` | Prometheus metrics |
| POST | `/api/v1/sessions` | Create session |
| GET | `/api/v1/sessions/{id}` | Get session |
| DELETE | `/api/v1/sessions/{id}` | Close session |
| POST | `/api/v1/sessions/{id}/messages` | Send message |
| GET | `/api/v1/sessions/{id}/history` | Message history |
| GET | `/api/v1/nous` | List agents |
| GET | `/api/v1/nous/{id}` | Agent status |
| GET | `/api/v1/nous/{id}/tools` | Agent tools |

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
