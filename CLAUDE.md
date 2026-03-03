# CLAUDE.md

Project conventions for AI coding agents working on this codebase.

**Ergon** is a fork of [forkwright/aletheia](https://github.com/forkwright/aletheia). Internal module names are preserved from upstream for merge compatibility.

## Standards

Follow [CONTRIBUTING.md](./CONTRIBUTING.md). Full reference: [docs/STANDARDS.md](docs/STANDARDS.md).

@.claude/rules/rust.md
@.claude/rules/typescript.md
@.claude/rules/svelte.md
@.claude/rules/python.md
@.claude/rules/architecture.md

## Structure

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full crate workspace, TypeScript module map, dependency graph, and trait boundaries.

### Config Locations

- **TS runtime:** `~/.aletheia/aletheia.json` — validated by Zod in `taxis/schema.ts`
- **Rust crates:** `instance/config/aletheia.yaml` — figment cascade (defaults → YAML → env vars)
- **Specs:** `docs/specs/` — design documents
- **Decisions:** `docs/decisions/` — Architecture Decision Records

## Commands

### Rust

```bash
cargo build                            # Debug build
cargo build --release                  # Release (LTO, stripped)
cargo test --workspace                 # All tests
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

## Key Patterns

### Rust

- **Errors:** `snafu` with `.context()` propagation and `Location` tracking
- **IDs:** Newtypes for all domain IDs (`AgentId`, `SessionId`, `NousId`)
- **Time:** `jiff` for time, `ulid` for IDs, `compact_str` for small strings
- **Async:** Tokio actor model (`NousActor` pattern)
- **Config:** figment YAML cascade in `taxis`

### TypeScript

- **Errors:** `AletheiaError` hierarchy in `koina/errors.ts`, `trySafe`/`trySafeAsync` in `koina/safe.ts`
- **Logging:** `createLogger("module-name")` with AsyncLocalStorage context
- **Events:** `eventBus` — `noun:verb` naming (e.g., `turn:before`, `tool:called`)
- **Config:** Zod schemas in `taxis/schema.ts`
- **Imports:** `.js` extensions, order: node → external → internal → local

### Both Stacks

- **Naming:** Greek names per [gnomon.md](docs/gnomon.md)
- **No barrel files** — import from the file that owns the symbol
- **Module imports flow downward** — higher layers depend on lower, never reverse

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

## Dianoia Gotchas

1. **Migration propagation:** Every `makeDb()` helper in `src/dianoia/*.test.ts` must include ALL migrations. When adding a migration, update ALL test helpers.
2. **exactOptionalPropertyTypes:** Use conditional spread (`...(value !== undefined ? { field: value } : {})`) not `field: value ?? undefined`.
3. **oxlint require-await:** Use `return Promise.resolve(result)` instead of `async` on functions with no `await`.
4. **Orchestrator registration:** New orchestrators follow the `NousManager` setter/getter pattern with conditional spread in `RouteDeps`.
