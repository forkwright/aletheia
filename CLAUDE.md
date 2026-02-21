# CLAUDE.md

Project conventions for AI coding agents working on this codebase.

## Standards

Follow [CONTRIBUTING.md](./CONTRIBUTING.md). Key points: self-documenting code, typed errors (`AletheiaError`), never empty catch, test behavior not implementation.

## Structure

- **Runtime:** `infrastructure/runtime/src/` — TypeScript, tsdown, vitest
- **UI:** `ui/` — Svelte 5, Vite
- **Memory sidecar:** `infrastructure/memory/sidecar/` — Python FastAPI
- **Config:** `~/.aletheia/aletheia.json` — validated by Zod in `taxis/schema.ts`
- **Specs:** `docs/specs/` — design documents numbered by implementation order

## Commands

```bash
cd infrastructure/runtime
npx vitest run                        # All tests
npx vitest run src/path/file.test.ts  # Specific test
npx tsdown                            # Build runtime
npx tsc --noEmit                      # Type check
npx oxlint src/                       # Lint
cd ../../ui && npm run build          # Build UI
aletheia doctor                       # Validate config
```

## Patterns

- **Modules:** Greek names — koina, taxis, mneme, hermeneus, nous, organon, semeion, pylon, prostheke
- **Errors:** `AletheiaError` hierarchy in `koina/errors.ts`, codes in `koina/error-codes.ts`, `trySafe`/`trySafeAsync` in `koina/safe.ts`
- **Logging:** `createLogger("module-name")` — structured with AsyncLocalStorage context
- **Events:** `eventBus` — `noun:verb` naming (e.g., `turn:before`, `tool:called`)
- **Config:** Zod schemas in `taxis/schema.ts`
- **Imports:** `.js` extensions, order: node → external → internal → local

## Before Submitting

1. Tests pass for affected files
2. No new empty catch blocks
3. New errors use typed error classes
4. `npx tsc --noEmit` clean
