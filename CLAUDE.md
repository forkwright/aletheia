# CLAUDE.md

Project conventions for AI coding agents working on this codebase.

**Ergon** is a fork of [forkwright/aletheia](https://github.com/forkwright/aletheia). Internal module names are preserved from upstream for merge compatibility.

## Standards

Follow [CONTRIBUTING.md](./CONTRIBUTING.md). Key points: self-documenting code, typed errors, never empty catch, test behavior not implementation.

@.claude/rules/rust.md
@.claude/rules/typescript.md
@.claude/rules/svelte.md
@.claude/rules/python.md
@.claude/rules/architecture.md

## Structure

### Rust Crate Workspace (target architecture)

11 application crates in `crates/`, plus 4 support crates:

| Crate | Purpose |
|-------|---------|
| `koina` | Errors (snafu), tracing, fs utilities, safe wrappers — leaf node |
| `taxis` | Config loading (figment YAML cascade), path resolution, oikos hierarchy |
| `mneme` | Unified memory store, embedding provider trait, knowledge retrieval |
| `mneme-engine` | CozoDB embedded database: vectors, graph, relations (vendored, ~42K lines) |
| `hermeneus` | Anthropic client, model routing, credentials, streaming retry, provider trait |
| `organon` | Tool registry, tool definitions, built-in tool set |
| `symbolon` | JWT tokens, password hashing, RBAC policies — leaf node |
| `nous` | Agent pipeline, NousActor (tokio), bootstrap, recall, execute, finalize |
| `melete` | Context distillation, compression, token budgets |
| `agora` | Channel registry, ChannelProvider trait, Signal + Slack providers |
| `pylon` | Axum HTTP gateway, SSE streaming, static UI serving |

Plus: `graph-builder` (build-time dep visualization), `integration-tests`, `mneme-bench`.

### TypeScript Runtime (current production)

Gateway source: `infrastructure/runtime/src/`

| Module | Purpose |
|--------|---------|
| `taxis/` | Config loading + Zod validation |
| `mneme/` | Session store (better-sqlite3, 10 migrations) |
| `hermeneus/` | Anthropic SDK + provider router |
| `organon/` | Tool registry + 48 built-in tools + skills |
| `semeion/` | Signal client + listener + commands |
| `pylon/` | Hono HTTP gateway, MCP, Web UI |
| `nous/` | Agent bootstrap + turn pipeline |
| `melete/` | Distillation, reflection, memory flush |
| `symbolon/` | Split-token authentication |
| `dianoia/` | Multi-phase planning orchestrator |
| `agora/` | Channel abstraction, Slack integration |
| `daemon/` | Cron, watchdog, update checker |
| `koina/` | Shared utilities |

### Other Components

| Path | What |
|------|------|
| `ui/` | Web UI (Svelte 5, Vite) |
| `infrastructure/memory/` | Mem0 sidecar (Python/FastAPI) + Qdrant + Neo4j |
| `infrastructure/prosoche/` | Adaptive attention daemon (Python) |
| `instance/` | Deployment state: agent workspaces, config, data |
| `shared/` | Scripts, templates, hooks, calibration |

## Development

```bash
cd infrastructure/runtime
npm install
npx tsdown                  # Build
npm run typecheck           # tsc --noEmit
npm run lint:check          # oxlint
```

**Never run `npm test` locally.** CI handles full test runs.

## Git

- Branch from `main`. Always squash merge.
- Author: `Cody Kickertz <cody.kickertz@gmail.com>`
- Commit format: `<type>: <description>` (feat, fix, refactor, docs, test, chore)

## Key Patterns

- **Typed errors:** All errors extend `AletheiaError`. Codes in `koina/error-codes.ts`.
- **trySafe/trySafeAsync:** Non-critical ops use safe wrappers from `koina/safe.ts`.
- **exactOptionalPropertyTypes:** Use conditional spread for optional fields.
- **oxlint require-await:** Use `Promise.resolve()` instead of `async` for sync tool handlers.

## Dianoia (Planning)

Multi-phase planning at `src/dianoia/`. Key patterns:
- Constructor-injected `Database.Database` and `dispatchTool`
- `OrThrow` pattern for required lookups
- All test `makeDb()` helpers must include ALL migrations

## Configuration

- Example: `config/aletheia.example.json`
- Schema: `infrastructure/runtime/src/taxis/schema.ts` (Zod, source of truth)
- Branding: Set `branding.name` in config to customize UI title

## Documentation

- [docs/QUICKSTART.md](docs/QUICKSTART.md): Setup
- [docs/CONFIGURATION.md](docs/CONFIGURATION.md): Config reference
- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md): Building and testing
- [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md): Production setup
- [docs/WORKSPACE_FILES.md](docs/WORKSPACE_FILES.md): Agent workspace reference
- [ALETHEIA.md](ALETHEIA.md): Upstream naming philosophy
