# Contributing to Aletheia

## Setup

```bash
git clone https://github.com/forkwright/aletheia.git
cd aletheia
```

### Rust

```bash
cargo build && cargo test --workspace && cargo clippy --workspace
```

Rust stable (2024 edition). Clippy bundled.

### TypeScript

```bash
cd infrastructure/runtime && npm install
git config core.hooksPath .githooks
```

Node.js >= 22.12. Optional: Docker/Podman (Qdrant, Neo4j, Langfuse), signal-cli, Chromium.

## Building

### Rust

```bash
cargo build                     # debug
cargo build --release           # release (thin LTO, stripped)
cargo clippy --workspace        # lint - zero warnings, pedantic
```

Workspace lints: pedantic clippy with select allows. `dbg!`, `todo!`, `unimplemented!` denied. `unsafe` denied workspace-wide.

### TypeScript

```bash
cd infrastructure/runtime && npx tsdown
```

Output: `dist/entry.mjs` (~450KB ESM bundle). Dev without building: `npm run dev`.

## Testing

### Rust

```bash
cargo test --workspace                          # all
cargo test -p aletheia-nous                     # single crate
cargo test -p aletheia-nous -- actor            # filter by name
cargo test -p aletheia-integration-tests        # cross-crate
```

Tests live alongside source in `#[cfg(test)] mod tests`. Integration tests in `crates/integration-tests/`.

### TypeScript

```bash
npx vitest run                          # all
npx vitest run src/path/file.test.ts    # specific
```

Tests live alongside source as `*.test.ts`. Integration tests use `.integration.test.ts`.

### Local Validation

```bash
npm run typecheck && npm run lint:check
```

**Never run `npm test` locally.** It executes the full integration suite which requires running services. Use `npx vitest run src/path/file.test.ts` for targeted tests.

### Pre-commit Hook

`.githooks/pre-commit` runs typecheck + lint on staged files. No tests - that's CI's job. Install: `git config core.hooksPath .githooks`.

## Adding Tools

Tools live in `src/organon/built-in/`. Each exports a `ToolHandler`:

```typescript
export const myTool: ToolHandler = {
  definition: {
    name: "my_tool",
    description: "What this tool does",
    input_schema: { type: "object", properties: { param: { type: "string" } }, required: ["param"] },
  },
  async execute(input: Record<string, unknown>, context: ToolContext): Promise<string> {
    return JSON.stringify({ result: input["param"] });
  },
};
```

Register in `src/aletheia.ts`. Categories: `"essential"` (always available) or `"available"` (on-demand, expires after 5 unused turns).

## CLI

```
aletheia start [--no-memory]     # start memory services + gateway
aletheia stop [--all]            # stop gateway (--all includes containers)
aletheia restart                 # restart gateway
aletheia logs [-f]               # follow logs
aletheia tui                     # terminal UI
aletheia status                  # live metrics
aletheia doctor                  # validate config + connectivity
aletheia send -a <id> -m <text>  # send message
aletheia sessions [-a agent]     # list sessions
aletheia update [version]        # self-update
aletheia cron list|trigger <id>  # manage cron
```

## API Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/health` | Health check |
| GET | `/api/status` | Agent list + version |
| GET | `/api/metrics` | Full metrics |
| GET | `/api/agents` | All agents |
| GET | `/api/agents/:id` | Agent detail |
| GET | `/api/sessions` | Session list |
| GET | `/api/sessions/:id/history` | Message history |
| POST | `/api/sessions/send` | Send message |
| POST | `/api/sessions/stream` | Streaming message (SSE) |
| POST | `/api/sessions/:id/archive` | Archive session |
| POST | `/api/sessions/:id/distill` | Trigger distillation |
| GET | `/api/events` | SSE event stream |
| GET | `/api/costs/summary` | Token usage + cost |
| GET | `/api/cron` | Cron jobs |
| POST | `/api/cron/:id/trigger` | Trigger cron job |
| GET | `/api/skills` | Skills directory |
| GET | `/api/config` | Config summary |

## Git

### Branches

| Type | Pattern | Example |
|------|---------|---------|
| Spec work | `spec<NN>/<description>` | `spec14/dev-workflow` |
| Feature | `feat/<description>` | `feat/gcal-rebuild` |
| Bug fix | `fix/<description>` | `fix/distillation-overflow` |
| Chore/docs | `chore/<description>` | `chore/readme-update` |

Branch from `main`. Rebase before pushing (`git pull --rebase origin main`). Never commit directly to `main` except docs-only or trivial config.

### Commits

Conventional commits: `<type>(<scope>): <description>`

Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `ci`, `perf`. Present tense imperative, first line <=72 chars, body wraps at 80 chars.

### Squash Policy

Always squash merge. Every PR becomes a single commit on `main`.

## Code Standards

Full reference: [docs/STANDARDS.md](docs/STANDARDS.md). Highlights:

- Self-documenting code. Comments only for *why*.
- Typed errors - `AletheiaError` (TS), `snafu` (Rust). Never throw strings or bare `Error`.
- No silent catch blocks.
- Greek naming for modules and crates (see [ALETHEIA.md](ALETHEIA.md)).
- Tests: behavior not implementation, descriptive names, same-directory files.

### Rust

- Edition 2024, `unsafe` denied, pedantic clippy
- Errors via `snafu` with context selectors
- `pub(crate)` by default
- `expect("invariant description")` over bare `unwrap()`

### TypeScript

- Strict mode with `exactOptionalPropertyTypes`, `noUncheckedIndexedAccess`
- Bracket notation for index access: `record["key"]`
- `.js` import extensions required

### Universal

- One-line file header max
- No inline comments except genuinely non-obvious *why* explanations
- No creation dates, author info, or AI generation indicators

## Reporting Issues

- **Bugs:** [bug report template](.github/ISSUE_TEMPLATE/bug_report.md)
- **Features:** [feature request template](.github/ISSUE_TEMPLATE/feature_request.md)
- **Security:** [SECURITY.md](SECURITY.md) - do not open public issues

## License

By contributing, you agree to [AGPL-3.0](LICENSE). This project follows the [Code of Conduct](CODE_OF_CONDUCT.md).
