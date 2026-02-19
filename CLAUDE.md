# CLAUDE.md

Project conventions for AI coding agents working on this codebase.

## Standards

All code must follow [CONTRIBUTING.md](./CONTRIBUTING.md). Key points:

- **Self-documenting code.** No "what" comments. Only "why" comments where non-obvious.
- **Typed errors.** All errors extend `AletheiaError`. Error codes in `koina/error-codes.ts`. Never throw strings or bare `Error`.
- **Never empty catch.** Every catch logs, rethrows, or returns a meaningful value.
- **Test behavior, not implementation.** One assertion per test. Descriptive names.

## Project Structure

- **Runtime:** `infrastructure/runtime/src/` — TypeScript, built with tsup, tested with vitest
- **UI:** `ui/` — Svelte 5, built with Vite
- **Memory sidecar:** `infrastructure/memory/sidecar/` — Python FastAPI
- **Config:** `~/.aletheia/aletheia.json` — validated by Zod schema in `taxis/schema.ts`
- **Specs:** `docs/specs/` — design documents numbered by implementation order

## Commands

```bash
# Run all tests
cd infrastructure/runtime && npx vitest run

# Run specific test file
cd infrastructure/runtime && npx vitest run src/path/to/file.test.ts

# Build runtime
cd infrastructure/runtime && npm run build

# Build UI
cd ui && npm run build

# Validate config
aletheia doctor

# Lint
cd infrastructure/runtime && npx eslint src/
```

## Key Patterns

- **Module naming:** Greek names (koina, taxis, mneme, hermeneus, nous, organon, semeion, pylon, prostheke)
- **Error handling:** `AletheiaError` hierarchy in `koina/errors.ts`, codes in `koina/error-codes.ts`
- **Logging:** `createLogger("module-name")` from `koina/logger.ts` — structured with AsyncLocalStorage context
- **Events:** `eventBus` from `koina/event-bus.ts` — `noun:verb` naming pattern
- **Config:** Zod schemas in `taxis/schema.ts`, loaded by `taxis/loader.ts`

## Before Submitting

1. Run tests for affected files
2. Ensure no new empty catch blocks
3. Check that new errors use typed error classes with codes
4. Verify import order follows the convention (node → external → internal → local)
