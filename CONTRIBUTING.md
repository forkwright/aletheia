# Contributing to Aletheia

## Setup

```bash
git clone https://github.com/forkwright/aletheia.git
cd aletheia
cargo build && cargo test --workspace && cargo clippy --workspace
```

Rust stable 1.85+ (edition 2024). Clippy bundled.

For an architectural walkthrough, see [ARCHITECTURE-GUIDE.md](docs/ARCHITECTURE-GUIDE.md). For the reference module map, see [ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Building

```bash
cargo build                     # debug
cargo build --release           # release (thin LTO, stripped)
cargo clippy --workspace        # lint — zero warnings, pedantic
```

Workspace lints: pedantic clippy with select allows. `dbg!`, `todo!`, `unimplemented!` denied. `unsafe` denied workspace-wide.

## Testing

```bash
cargo test --workspace                          # all
cargo test -p aletheia-nous                     # single crate
cargo test -p aletheia-nous -- actor            # filter by name
cargo test -p aletheia-integration-tests        # cross-crate
```

Tests live alongside source in `#[cfg(test)] mod tests`. Integration tests in `crates/integration-tests/`.

During development, use targeted tests + clippy. Full suite as a final gate before PR:

```bash
# During development
cargo test -p <crate-being-modified>
cargo clippy --workspace --all-targets -- -D warnings

# Final gate
cargo test --workspace
```

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

### Branches

| Type | Pattern | Example |
|------|---------|---------|
| Feature | `feat/<description>` | `feat/recall-pipeline` |
| Bug fix | `fix/<description>` | `fix/session-timeout` |
| Docs | `docs/<description>` | `docs/deployment-guide` |
| Refactor | `refactor/<description>` | `refactor/config-cascade` |
| Chore | `chore/<description>` | `chore/update-deps` |
| Test | `test/<description>` | `test/mneme-roundtrip` |

Branch from `main`. Rebase before pushing (`git pull --rebase origin main`). Never commit directly to `main`.

### Commits

Conventional commits: `<type>(<scope>): <description>`

Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `ci`, `perf`. Present tense imperative, first line <=72 chars.

Scope is the crate name: `feat(nous): add history stage`, `fix(mneme): graph score aggregation`.

### Squash Policy

Always squash merge. Every PR becomes a single commit on `main`.

## Code Standards

Full reference: [docs/STANDARDS.md](docs/STANDARDS.md). Highlights:

- Self-documenting code. Comments only for *why*.
- Errors via `snafu` with context selectors. Never throw strings or bare `Error`.
- `pub(crate)` by default. `pub` only for cross-crate API.
- `#[expect(lint, reason = "...")]` over `#[allow]`.
- `unsafe` denied workspace-wide.
- Greek naming for modules and crates (see [ALETHEIA.md](ALETHEIA.md)).
- Tests: behavior not implementation, descriptive names, same-directory files.
- Edition 2024: native async traits, let chains, `LazyLock` over `once_cell`.

## Reporting Issues

- **Bugs:** [bug report template](.github/ISSUE_TEMPLATE/bug_report.md)
- **Features:** [feature request template](.github/ISSUE_TEMPLATE/feature_request.md)
- **Security:** [SECURITY.md](SECURITY.md) — do not open public issues

## License

By contributing, you agree to [AGPL-3.0](LICENSE). This project follows the [Code of Conduct](CODE_OF_CONDUCT.md).
