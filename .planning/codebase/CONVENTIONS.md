# Coding Conventions

**Analysis Date:** 2026-02-24

## Naming Patterns

**Files:**
- `kebab-case.ts` for implementation and test files
- Pattern: `module-name.ts`, `module-name.test.ts`, `module-name.integration.test.ts`
- Examples: `error-codes.ts`, `event-bus.ts`, `store.test.ts`

**Functions:**
- `camelCase`, verb-first prefix
- Examples: `createLogger()`, `findSession()`, `runBufferedPipeline()`, `withTurnAsync()`
- Boolean functions use `is`/`has`/`should` prefix: `findSession()`, `matchesToolFilter()`

**Variables:**
- `camelCase` for regular variables
- `UPPER_SNAKE` for module-level constants
- Examples: `turnCounter`, `sessionLocks`, `DEFAULT_MAX_RESULT_TOKENS`, `EXPIRY_TURNS`

**Types and Classes:**
- `PascalCase` for classes and interfaces
- Examples: `AletheiaError`, `ConfigError`, `SessionError`, `ToolRegistry`, `EventBus`

**Events:**
- `noun:verb` naming convention
- Examples: `turn:before`, `turn:after`, `tool:called`, `tool:failed`, `session:created`, `distill:before`
- See `koina/event-bus.ts` for canonical event names

**Modules:**
- Greek names (thematic, not in code): `taxis`, `mneme`, `hermeneus`, `organon`, `nous`, `distillation`, `semeion`, `pylon`, `prostheke`, `daemon`, `koina`
- Initialize in order: `taxis → mneme → hermeneus → organon → nous → distillation → semeion → pylon → prostheke → daemon`

## Code Style

**Formatting:**
- oxlint (Rust-based linter) enforces style
- Run: `npm run lint:check` (check only), `npm run lint` (fix)
- No Prettier — oxlint handles all formatting

**Linting:**
- Tool: `oxlint v1.50.0+`
- Config: `oxlint src/`
- Errors must be fixed before precommit passes

**TypeScript Configuration:**
- Strict mode enabled
- `exactOptionalPropertyTypes: true` — optional properties cannot be `undefined`
- `noUncheckedIndexedAccess: true` — bracket notation requires type narrowing
- `noPropertyAccessFromIndexSignature: true` — cannot access properties known only through index signature
- `noUnusedLocals: true` and `noUnusedParameters: true` — all code must be used
- `noFallthroughCasesInSwitch: true` — all switch cases must have explicit return/break

## Import Organization

**Order:**
```typescript
import { join } from "node:path";                           // 1. Node builtins (node: prefix)
import { Logger } from "tslog";                             // 2. External packages
import type { Hono } from "hono";                           // 3. External types
import { createLogger } from "../koina/logger.js";          // 4. Internal modules
import { AletheiaError } from "../koina/errors.js";
import type { TurnState } from "./pipeline/types.js";       // 5. Local types
```

**Rules:**
- Always use `.js` file extensions (even in TypeScript source)
- Separate `import` and `import type` sections
- Group by: node builtins → external → internal → local
- Within group: alphabetical

**Examples from codebase:**
- `nous/manager.ts` starts with node builtins, then external, then internal modules
- `koina/event-bus.ts` shows clean external → internal pattern

## Error Handling

**Typed Errors:**
- All errors extend `AletheiaError` from `koina/errors.ts`
- Use error code registry from `koina/error-codes.ts`
- Format: `MODULE_CONDITION` (e.g., `PROVIDER_TIMEOUT`, `SESSION_NOT_FOUND`)

**Error Class Hierarchy:**
- Base: `AletheiaError(opts: AletheiaErrorOpts)` — includes code, module, message, context, recoverable, retryAfterMs
- `ConfigError` — module: `"taxis"`, default code: `"CONFIG_VALIDATION_FAILED"`
- `SessionError` — module: `"mneme"`, default code: `"SESSION_NOT_FOUND"`
- `ProviderError` — module: `"hermeneus"`, default code: `"PROVIDER_TIMEOUT"`, supports `recoverable` and `retryAfterMs`
- `ToolError` — module: `"organon"`, default code: `"TOOL_EXECUTION_FAILED"`
- `PipelineError` — module: `"nous"`, default code: `"PIPELINE_STAGE_FAILED"`, supports `recoverable`
- `TransportError` — module: `"semeion"`, default code: `"SIGNAL_SEND_FAILED"`, supports `recoverable` and `retryAfterMs`

**Throwing Errors:**
```typescript
throw new PipelineError("Stage failed", {
  code: "PIPELINE_STAGE_FAILED",
  context: { stage, sessionId },
  recoverable: true,
});
```

**Non-Critical Operations:**
- Use `trySafe<T>(label, fn, fallback)` from `koina/safe.ts` for synchronous operations
- Use `trySafeAsync<T>(label, fn, fallback)` for async operations
- Pattern: `const result = trySafe("skill extraction", () => extractSkill(data), null);`
- Logs warning with operation label if error occurs, returns fallback value
- Example: `koina/safe.ts`, `koina/hooks.ts`

**No Silent Catch:**
- Every catch block must either:
  1. Log the error, OR
  2. Rethrow the error, OR
  3. Return a meaningful value, OR
  4. Include an inline comment explaining why error is intentionally discarded

**Example from code:**
```typescript
catch (err) {
  log.warn(`${label} failed (non-fatal): ${err instanceof Error ? err.message : err}`);
  return fallback;
}

// Or with comment:
catch { /* log write failed — cannot recurse */
  // Don't recurse on log failure
}
```

## Logging

**Framework:** `tslog` (v4.9.3+)

**Creating a Logger:**
```typescript
import { createLogger } from "../koina/logger.js";
const log = createLogger("module-name");
```

**Structured Output:**
- Pretty-printed console (default)
- Optional JSON transport to file via `ALETHEIA_LOG_JSON` env var
- Includes turn context via AsyncLocalStorage: `turnId`, `nousId`, `sessionId`, `sessionKey`, `channel`, `sender`

**Log Levels:**
- Environment: `ALETHEIA_LOG_LEVEL` (default: `"info"`)
- Per-module override: `ALETHEIA_LOG_MODULES` (format: `"semeion:debug,nous:trace,hermeneus:info"`)

**Usage:**
```typescript
log.info("message");
log.warn("warning");
log.error("error");
log.debug("debug only when enabled");
```

**Turn Context Propagation:**
- Automatic via `withTurn()` or `withTurnAsync()` in `koina/logger.ts`
- All logs inherit context within that scope

## File Headers

**Pattern:** One-line comment at top
```typescript
// Module name — what this file does
```

**Examples:**
- `// Structured error hierarchy for machine-readable error handling`
- `// Structured logging with request tracing via AsyncLocalStorage`
- `// Cron scheduler tests`
- `// Pipeline runner — composes stages for streaming and non-streaming turn execution`

**Rules:**
- Present tense, concise
- No creation dates, author info, or AI generation indicators
- No multi-line headers
- Immediately followed by imports

## Comments

**When to Comment:**
- Explain "why", not "what"
- Complex algorithms, non-obvious logic, architectural decisions
- Inline comments only for true complexity

**Example from codebase:**
```typescript
// Patch target is in a forbidden directory
PATCH_FORBIDDEN_PATH: "Patch target is in a forbidden directory",
```

**Avoid:**
- Comments restating obvious code: `const x = 5; // set x to 5`
- Comments on every line
- Commented-out code blocks

**JSDoc/TSDoc:**
- Not required for obvious functions
- Use for public APIs and complex signatures
- Pattern: match tslog style (minimal, functional)

## Function Design

**Size:** Single responsibility, typically 10-50 lines
- `createLogger()` — 10 lines
- `withTurnAsync()` — 5 lines
- `runStreamingPipeline()` — generator, ~70 lines (complex pipeline orchestration)

**Parameters:**
- Prefer single object for related options: `opts?: StreamingPipelineOpts`
- Typed destructuring in destructors

**Return Values:**
- Explicit return types always
- Prefer `T | undefined` over `null` unless `null` is semantically meaningful
- Use typed unions for multiple return types

**Examples:**
```typescript
export async function withTurnAsync<T>(
  ctx: Partial<TurnContext>,
  fn: () => Promise<T>,
): Promise<T>

export function matchesToolFilter(name: string, patterns: string[]): boolean
```

## Module Design

**Exports:**
- Explicit named exports, no default exports
- Group logical related exports
- Pattern: implementation first, types last

**Barrel Files:**
- Minimal use — export only true public APIs
- Not used in most modules

**Module Coupling:**
- Unidirectional dependencies: downstream modules import upstream
- Example: `nous` imports `hermeneus`, but not vice versa
- Avoid circular imports

**Configuration Management:**
- Zod schemas in `taxis/schema.ts` — single source of truth
- Validators must pass before use
- Example: `taxis/schema.ts` has `ModelSpec`, `HeartbeatConfig`, `RoutingConfig`, `CompactionConfig`

## Index Access

**Bracket Notation:**
- Always use bracket notation for index access: `record["key"]`, `arr[0]`
- With `noUncheckedIndexedAccess: true`, type narrowing required
- Example from codebase: `callArgs = (router.complete as ReturnType<typeof vi.fn>).mock.calls[0]![0]`

**Type Safety:**
- Index results are typed as `T | undefined` (unless narrowed)
- Non-null assertion `!` only after narrowing or certainty

## Event System

**Event Bus Pattern:**
- Centralized `eventBus` from `koina/event-bus.ts`
- Fire-and-forget pub/sub
- Async handlers don't block emitter

**Usage:**
```typescript
import { eventBus, type EventName } from "../koina/event-bus.js";

eventBus.on("turn:after", (payload) => {
  // Handle event
});

eventBus.emit("turn:after", { sessionId, nousId, inputTokens: 150 });
```

**Event Names:** Canonical list in `event-bus.ts` — `turn:before`, `turn:after`, `tool:called`, `tool:failed`, etc.

---

*Conventions analysis: 2026-02-24*
