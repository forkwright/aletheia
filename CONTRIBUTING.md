# Contributing to Aletheia

## Getting Started

1. Fork the repository
2. Clone your fork and set up the dev environment:
   ```bash
   cd infrastructure/runtime
   npm install
   git config core.hooksPath .githooks
   ```
3. Create a branch from `main`

## Development

### Build

```bash
cd infrastructure/runtime
npx tsdown
```

### Test

```bash
npm test                    # Unit tests
npm run test:coverage       # With coverage thresholds
npm run test:integration    # Integration tests (30s timeout)
```

### Lint & Type Check

```bash
npm run typecheck           # tsc --noEmit
npm run lint:check          # oxlint
npm run precommit           # All checks (typecheck + lint + test)
```

### Pre-commit Hook

The hook runs `typecheck` and `lint:check` automatically. Enable it with:

```bash
git config core.hooksPath .githooks
```

## Pull Requests

- Keep PRs focused — one feature or fix per PR
- Include tests for new functionality
- Ensure `npm run precommit` passes
- Write a clear description of what changed and why
- Reference the spec number for spec work
- Reference related issues with `Fixes #123` or `Closes #123`

---

## Code Standards

### Self-Documenting Code Over Comments

**File headers:** Each file gets one header comment — a single line explaining what the module is:

```typescript
// Pipeline runner — composes stages for streaming and non-streaming turn execution
```

**Inline comments:** Only where the *why* is non-obvious. Never comment *what* the code does.

Good:
```typescript
// SQLCipher 4 format — must be set before any other pragma
this.db.pragma(`key = '${encryptionKey}'`);
```

Bad:
```typescript
// Get the session
const session = store.findSessionById(sessionId);
```

If the code needs a "what" comment to be understood, rename the variables and functions until it doesn't.

### Naming

| Thing | Convention | Example |
|-------|-----------|---------|
| Files | kebab-case | `build-messages.ts`, `session-store.ts` |
| Classes | PascalCase | `SessionStore`, `ToolRegistry` |
| Functions | camelCase, verb-first | `resolveThread`, `buildContext`, `parseConfig` |
| Constants | UPPER_SNAKE | `MAX_CONCURRENT_TURNS`, `SIDECAR_URL` |
| Types/Interfaces | PascalCase | `TurnState`, `DistillationOpts` |
| Booleans | `is`/`has`/`should` prefix | `isStreaming`, `hasToken`, `shouldDistill` |
| Event names | `noun:verb` or `noun:adjective` | `turn:before`, `distill:after`, `tool:called` |

### TypeScript

- **Strict mode** — all strict flags enabled, `exactOptionalPropertyTypes`, `noUncheckedIndexedAccess`
- **Imports** — use `.js` extensions (NodeNext resolution)
- **Index access** — bracket notation for string-keyed records (`record["key"]`, not `record.key`)

### Function Design

- **Single responsibility.** If a function needs a comment explaining its sections, split it.
- **Early returns over nested ifs.** Guard clauses at the top, happy path below.
- **Explicit over clever.** The next person reading this code is a tired engineer at 2am.
- **Right tool for the job.** Don't use `reduce` when a `for` loop is clearer. Don't use a class when a function suffices. Don't use generics when the type is always the same.

### Import Order

```typescript
// 1. Node built-ins
import { join } from "node:path";
import { readFileSync } from "node:fs";

// 2. External packages
import { Hono } from "hono";

// 3. Internal absolute imports (by module)
import { createLogger } from "../koina/logger.js";
import type { SessionStore } from "../mneme/store.js";

// 4. Local relative imports
import { buildMessages } from "./utils/build-messages.js";
import type { TurnState } from "./types.js";
```

---

## Error Handling

### Never Empty Catch

Every `catch` block either logs, rethrows, or returns a meaningful value. No exceptions.

```typescript
// ❌ Never
try { doThing(); } catch {}
promise.catch(() => {});

// ✅ Always
try { doThing(); } catch (err) {
  log.warn(`doThing failed (non-fatal): ${err instanceof Error ? err.message : err}`);
}
```

### Use Typed Errors

All errors extend `AletheiaError` from `koina/errors.ts`. Error codes are defined in `koina/error-codes.ts`.

```typescript
import { PipelineError } from "../koina/errors.js";

throw new PipelineError("Stage failed", {
  code: "PIPELINE_STAGE_FAILED",
  context: { stage: "context", sessionId },
  recoverable: true,
});
```

Never throw strings. Never throw bare `Error`.

### Error Boundaries for Non-Critical Operations

Optional operations (skill learning, interaction signals, workspace flush) use `trySafe` / `trySafeAsync`:

```typescript
import { trySafe, trySafeAsync } from "../koina/safe.js";

// Sync
const result = trySafe("skill extraction", () => extractSkill(data), null);

// Async
const mem = await trySafeAsync("memory flush", () => flushToMemory(target), { errors: 0 });
```

The intent is explicit: "this operation is optional and must not crash the caller."

### Log at the Boundary

The function that catches the error logs it. Inner functions let errors propagate.

---

## Testing

- **Test behavior, not implementation.** Tests break when the contract changes, not when internals refactor.
- **One assertion per test** where practical. Name describes the assertion.
- **Test names:** `it("returns null when session not found")` — not `it("test 1")`.
- **No internal state access.** If you need `(store as any).db.prepare(...)`, the store needs a method for that.
- **Test files:** Same directory as the module, named `module.test.ts` (vitest with `describe`/`it`).

---

## Module Architecture

The runtime is organized by Greek-named subsystems:

| Module | Domain | Examples |
|--------|--------|---------|
| `koina` | Shared infrastructure | Logger, errors, event bus, crypto, filesystem |
| `taxis` | Configuration | Schema, loader, paths |
| `mneme` | Memory/storage | Session store, message persistence |
| `hermeneus` | LLM providers | Anthropic client, router, pricing, complexity |
| `nous` | Agent management | Bootstrap, pipeline, manager |
| `organon` | Tools | Built-in tools, tool registry |
| `semeion` | Signal transport | Listener, sender, TTS |
| `pylon` | Gateway/HTTP | Server, routes, middleware |
| `prostheke` | Plugins | Loader, registry, hooks |
| `distillation` | Context compression | Pipeline, extraction, summarization |
| `daemon` | Background tasks | Cron, watchdog, retention |
| `auth` | Authentication | JWT, RBAC, sessions (partially wired) |

New code goes in the appropriate subsystem. If none fits, it probably belongs in `koina` (shared) or needs a new subsystem.

### Adding Tools

Tools live in `src/organon/built-in/`. See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md#adding-new-tools) for the full guide.

### Adding Commands

Signal commands (`!command`) are registered in `src/semeion/commands.ts`. See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md#adding-new-built-in-commands).

---

## Architecture Principles

1. **Pipeline stages are composable.** Each stage gets TurnState in, returns TurnState out. Side effects are explicit.
2. **Config is the source of truth.** Runtime behavior comes from `aletheia.json` via Zod-validated schema. No magic environment variables for core config.
3. **Events over callbacks.** Use the `eventBus` for cross-cutting concerns. Don't pass callback chains through 5 layers.
4. **Errors carry context.** Every error has a code, a module, and enough context to debug without reading the source.
5. **Agents are independent.** Each nous has its own workspace, identity, and config. Shared state goes through the store or event bus.

---

## Git Conventions

- **Commits:** Descriptive, present tense. `fix: prevent orphan messages on pipeline error`
- **Branches:** `spec<N>-<description>` for spec work, `fix/<description>` for bugs
- **Always push after commit.** `git commit && git push` — never leave commits local.

---

## Reporting Issues

- **Bugs** — use the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md)
- **Features** — use the [feature request template](.github/ISSUE_TEMPLATE/feature_request.md)
- **Security** — see [SECURITY.md](.github/SECURITY.md). Do not open public issues for vulnerabilities.

## License

By contributing, you agree that your contributions will be licensed under the [AGPL-3.0](LICENSE).
