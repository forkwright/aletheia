# CLAUDE.md

Project conventions for AI coding agents working on this codebase.

## Standards

Universal: [standards/README.md](standards/README.md)
Rust: [standards/RUST.md](standards/RUST.md)
Writing: [standards/WRITING.md](standards/WRITING.md)
Shell: [standards/SHELL.md](standards/SHELL.md)

## Structure

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): crate workspace, module map, dependency graph, trait boundaries.

### Config

- **Rust crates:** `instance/config/aletheia.toml` (figment cascade: defaults → TOML → env vars)

## Commands

```bash
cargo build                            # Debug build
cargo build --release                  # Release (LTO, stripped)
cargo test --workspace                 # All tests
cargo test -p aletheia-hermeneus       # Single crate
cargo clippy --workspace               # Lint (zero warnings)
```

## Key patterns

- **Errors:** `snafu` with `.context()` propagation and `Location` tracking
- **IDs:** Newtypes for all domain IDs (`AgentId`, `SessionId`, `NousId`)
- **Time:** `jiff` for time, `ulid` for IDs, `compact_str` for small strings
- **Async:** Tokio actor model (`NousActor` pattern)
- **Config:** figment TOML cascade in `taxis`
- **Lints:** `#[expect(lint, reason = "...")]` over `#[allow]`; every suppression justified
- **Visibility:** `pub(crate)` by default; `pub` only for cross-crate API surface
- **Naming:** Greek names per [docs/gnomon.md](docs/gnomon.md), registry at [docs/lexicon.md](docs/lexicon.md)
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

## CLI

| Subcommand | Description |
|------------|-------------|
| *(none)* | Start the server |
| `health [--url URL]` | HTTP health check against a running instance |
| `status [--url URL]` | System status dashboard |
| `backup [--list\|--prune --keep N\|--export-json]` | Database backup management |
| `maintenance status` | Show status of all maintenance tasks |
| `maintenance run <task> [--verbose]` | Run task: `trace-rotation`, `drift-detection`, `db-monitor`, `all` |
| `tls generate [--output-dir PATH] [--days N] [--san NAMES...]` | Generate self-signed TLS certificates |
| `credential status` | Show credential source, expiry, and token prefix |
| `credential refresh` | Force-refresh OAuth token |
| `eval [--url URL] [--token TOKEN] [--scenario ID] [--json] [--timeout N]` | Run behavioral evaluation scenarios |
| `session-export <session-id> [--format md\|json] [--output PATH] [--url URL] [--token TOKEN]` | Export a session as Markdown or JSON |
| `export <nous-id> [--output PATH] [--archived] [--max-messages N] [--compact]` | Export agent to `.agent.json` |
| `import <file> [--target-id ID] [--skip-sessions] [--skip-workspace] [--force] [--dry-run]` | Import agent from `.agent.json` |
| `tui [--url URL] [--token TOKEN] [--agent ID] [--session ID] [--logout]` | Launch terminal dashboard (requires `tui` feature) |
| `seed-skills --dir PATH --nous-id ID [--force] [--dry-run]` | Seed skills from `SKILL.md` files into knowledge store |
| `export-skills --nous-id ID [--output PATH] [--domain TAGS]` | Export skills to Claude Code `.claude/skills/` format |
| `review-skills --nous-id ID [--action list\|approve\|reject] [--fact-id ID]` | Review pending auto-extracted skills |
| `migrate-memory [--qdrant-url URL] [--collection NAME] [--knowledge-path PATH] [--review-file PATH] [--dry-run]` | Migrate memories from Qdrant into embedded knowledge store |
| `add-nous <name> [--provider PROVIDER] [--model MODEL]` | Scaffold a new nous agent directory |
| `init [--instance-root\|--instance-path PATH] [-y] [--non-interactive] [--auth-mode MODE] [--api-provider PROVIDER] [--model MODEL] [--api-key KEY]` | Initialize a new instance |
| `check-config` | Validate configuration without starting services |
| `completions <bash\|zsh\|fish>` | Generate shell completions |

## API endpoints

### Infrastructure

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/health` | Liveness and readiness check |
| GET | `/api/docs/openapi.json` | OpenAPI spec |
| GET | `/metrics` | Prometheus metrics |

### Sessions

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/v1/sessions` | List sessions (query: `nous_id`) |
| POST | `/api/v1/sessions` | Create session |
| POST | `/api/v1/sessions/stream` | Stream a turn (TUI webchat protocol) |
| GET | `/api/v1/sessions/{id}` | Get session |
| DELETE | `/api/v1/sessions/{id}` | Close (archive) session |
| POST | `/api/v1/sessions/{id}/archive` | Archive session |
| POST | `/api/v1/sessions/{id}/unarchive` | Reactivate archived session |
| PUT | `/api/v1/sessions/{id}/name` | Rename session |
| POST | `/api/v1/sessions/{id}/messages` | Send message, SSE stream response |
| GET | `/api/v1/sessions/{id}/history` | Conversation history (query: `limit`, `before`) |
| GET | `/api/v1/events` | Global SSE event channel (TUI dashboard) |

### Nous (agents)

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/v1/nous` | List registered agents |
| GET | `/api/v1/nous/{id}` | Agent status and config |
| GET | `/api/v1/nous/{id}/tools` | Tools available to agent |

### Config

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/v1/config` | Full redacted runtime config |
| GET | `/api/v1/config/{section}` | Single config section |
| PUT | `/api/v1/config/{section}` | Update and persist config section |

### Knowledge

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/v1/knowledge/facts` | List facts (query: `nous_id`, `sort`, `order`, `filter`, `fact_type`, `tier`, `limit`, `offset`, `include_forgotten`) |
| GET | `/api/v1/knowledge/facts/{id}` | Fact detail with relationships and similar facts |
| POST | `/api/v1/knowledge/facts/{id}/forget` | Forget a fact |
| POST | `/api/v1/knowledge/facts/{id}/restore` | Restore a forgotten fact |
| PUT | `/api/v1/knowledge/facts/{id}/confidence` | Update fact confidence |
| GET | `/api/v1/knowledge/entities` | List entities |
| GET | `/api/v1/knowledge/entities/{id}/relationships` | Entity relationships |
| GET | `/api/v1/knowledge/search` | Full-text search (query: `q`, `nous_id`, `limit`) |
| GET | `/api/v1/knowledge/timeline` | Fact activity timeline |

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
