# CLAUDE.md

Project conventions for AI coding agents working on this codebase.

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
| `melete` | Context distillation, compression strategies, token budget management |
| `agora` | Channel registry, ChannelProvider trait, Signal JSON-RPC client |
| `pylon` | Axum HTTP gateway, SSE streaming, static UI serving, auth middleware |
| `aletheia` | Binary entrypoint (Clap CLI) — wires all crates together |
| `graph-builder` | CSR graph construction/traversal (build tool) |
| `integration-tests` | Cross-crate integration tests |
| `mneme-bench` | CozoDB validation benchmarks (excluded from default build) |

### TypeScript Runtime (current production)

- **Runtime:** `infrastructure/runtime/src/` — TypeScript, tsdown, vitest
- **UI:** `ui/` — Svelte 5, Vite
- **Memory sidecar:** `infrastructure/memory/sidecar/` — Python FastAPI

### Config

- **TS runtime:** `~/.aletheia/aletheia.json` — validated by Zod in `taxis/schema.ts`
- **Rust crates:** `instance/config/aletheia.yaml` — figment cascade (defaults → YAML → env vars)

### Other

- **Specs:** `docs/specs/` — design documents numbered by implementation order
- **Decisions:** `docs/decisions/` — Architecture Decision Records

## Commands

### Rust

```bash
cargo build                            # Debug build
cargo build --release                  # Release (LTO, stripped)
cargo test --workspace                 # All tests (747 across 14 crates)
cargo test -p aletheia-hermeneus       # Single crate
cargo clippy --workspace               # Lint (zero warnings policy)
```

### TypeScript

```bash
cd infrastructure/runtime
npx vitest run                         # All tests
npx vitest run src/path/file.test.ts   # Specific test
npx tsdown                             # Build runtime
npx tsc --noEmit                       # Type check
npx oxlint src/                        # Lint
cd ../../ui && npm run build           # Build UI
aletheia doctor                        # Validate config
```

## Patterns

### Rust

- **Errors:** `snafu` enums per crate with `.context()` propagation and `Location` tracking. See `.claude/rules/rust.md`.
- **IDs:** Newtypes for all domain IDs (`AgentId`, `SessionId`, `NousId`, etc.)
- **Time:** `jiff` for time, `ulid` for IDs, `compact_str` for small strings
- **Async:** Tokio actor model (`NousActor` pattern)
- **Config:** figment YAML cascade in `taxis`

### TypeScript

- **Modules:** Greek names — koina, taxis, mneme, hermeneus, nous, organon, melete, symbolon, dianoia, semeion, pylon, prostheke
- **Errors:** `AletheiaError` hierarchy in `koina/errors.ts`, codes in `koina/error-codes.ts`, `trySafe`/`trySafeAsync` in `koina/safe.ts`
- **Logging:** `createLogger("module-name")` — structured with AsyncLocalStorage context
- **Events:** `eventBus` — `noun:verb` naming (e.g., `turn:before`, `tool:called`)
- **Config:** Zod schemas in `taxis/schema.ts`
- **Imports:** `.js` extensions, order: node → external → internal → local

### Both Stacks

- **Naming:** Greek names per [gnomon.md](docs/gnomon.md). Names identify modes of attention, not implementations.
- **No barrel files** — import from the file that owns the symbol
- **Module imports flow downward** — higher layers may depend on lower, never the reverse

## Before Submitting

### Rust
1. `cargo test -p <affected-crate>` passes
2. `cargo clippy --workspace` — zero warnings
3. No `unwrap()` in library code
4. New errors use snafu with context

### TypeScript
1. Tests pass for affected files
2. No new empty catch blocks
3. New errors use typed error classes
4. `npx tsc --noEmit` clean
