# Coding Conventions

**Analysis Date:** 2026-02-23

## Naming Patterns

**Files:**
- Kebab-case: `retention.ts`, `evolution-cron.ts`, `token-counter.ts`
- Test files: `*.test.ts` for unit tests, `*.integration.test.ts` for integration tests, `*-full.test.ts` for full-suite tests
- Modules named with Greek words: `koina` (common), `taxis` (taxonomy), `mneme` (memory), `hermeneus` (interpretation), `nous` (mind), `organon` (tool), `semeion` (signal), `pylon` (gate), `prostheke` (addition)

**Functions:**
- camelCase, verb-first: `estimateTokens()`, `extractFromMessages()`, `createLogger()`, `findBalancedBraces()`
- Factory functions: `makeConfig()`, `makeStore()` in tests (local helpers)
- Async functions use same naming: `withTurnAsync()`, `trySafeAsync()`

**Variables:**
- camelCase for locals and parameters: `turnId`, `sessionKey`, `messageCount`
- const for immutable values: `CHARS_PER_TOKEN = 3.5`, `LEVEL_MAP`, `MESSAGE_OVERHEAD_TOKENS`
- UPPER_SNAKE for true constants: `ALETHEIA_LOG_LEVEL`, `ALETHEIA_LOG_MODULES`

**Types:**
- PascalCase for classes and interfaces: `NousManager`, `AletheiaError`, `SessionStore`, `ProviderRouter`
- Exported error classes inherit from `AletheiaError`: `ConfigError`, `SessionError`, `ProviderError`, `ToolError`, `PipelineError`, `TransportError`
- Readonly properties in interfaces: `readonly code: ErrorCode`, `readonly module: string`

**Events:**
- Noun:verb pattern: `turn:before`, `turn:after`, `distill:after`, `tool:called`

## Code Style

**Formatting:**
- TypeScript strict mode enabled
- Target: ES2022
- Module resolution: NodeNext
- Declaration maps and source maps generated for debugging
- File headers: One-line comment only, no author/date info

Example header:
```typescript
// Structured error hierarchy for machine-readable error handling
// Pass 1: Extract structured facts, decisions, and open items from conversation
// Token estimation with safety margins for budget calculations
```

**Linting:**
- Tool: oxlint with TypeScript and import plugins
- Key rules enforced:
  - `eqeqeq`: error — strict equality required
  - `no-empty`: error — never empty catch/function blocks
  - `typescript/no-floating-promises`: error — all promises must be awaited or explicitly returned
  - `typescript/no-misused-promises`: error — promises in conditionals must be handled
  - `typescript/consistent-type-imports`: error — separate type imports (e.g., `import type { Foo } from "..."`)
  - `import/no-duplicates`: error — combine imports from same module
  - `no-unused-vars`: warn
  - `typescript/no-explicit-any`: warn

Config location: `.oxlintrc.json`

Run linting:
```bash
npx oxlint src/                    # Check
npm run lint                       # Fix
npm run lint:check                 # CI gate
```

## Import Organization

**Order:**
1. Node built-ins: `import { createLogger } from "node:fs"`
2. External packages: `import { Logger } from "tslog"`
3. Internal modules: `import { createLogger } from "../koina/logger.js"`
4. Sibling/local imports: `import { retentionResult } from "./retention.js"`

**Path extensions:**
- REQUIRED: `.js` extensions on all imports (even in TypeScript)
  ```typescript
  import { createLogger } from "../koina/logger.js";  // ✓ correct
  import { createLogger } from "../koina/logger";      // ✗ incorrect
  ```

**Type imports:**
- Must use `import type` for type-only imports
  ```typescript
  import type { SessionStore } from "../mneme/store.js";
  import type { PrivacySettings } from "../taxis/schema.js";
  ```

**Grouping:**
- Separate groups with blank lines
- Keep groups organized (node, external, internal, local)

## Error Handling

**Pattern: Always use AletheiaError hierarchy**

All errors must extend `AletheiaError` (from `koina/errors.ts`). Error codes defined in `koina/error-codes.ts`.

```typescript
import { PipelineError } from "../koina/errors.js";

throw new PipelineError(
  "Failed to complete distillation",
  { code: "DISTILLATION_TIMEOUT", context: { duration: 30000 } }
);
```

Error subclasses for specific modules:
- `ConfigError`: Configuration validation failures (module: taxis)
- `SessionError`: Session not found/invalid (module: mneme)
- `ProviderError`: API/model provider failures (module: hermeneus) — includes `recoverable` and `retryAfterMs`
- `ToolError`: Tool execution failure (module: organon)
- `PipelineError`: Distillation pipeline failures (module: nous) — includes `recoverable`
- `TransportError`: Message transport failure (module: semeion) — includes `recoverable` and `retryAfterMs`

**Catch block rules (CRITICAL):**
- Never empty catch: `catch (err) { }` is forbidden
- Every catch must either:
  1. Log and rethrow
  2. Log and return a fallback value
  3. Log at appropriate level (error/warn)
  4. Include an inline comment explaining intentional discard: `catch { /* non-fatal */ }`

Example from `koina/logger.ts`:
```typescript
try {
  appendFileSync(jsonLogPath, JSON.stringify(entry) + "\n");
} catch { /* log write failed — cannot recurse */
  // Don't recurse on log failure
}
```

**Non-critical operations: Use trySafe/trySafeAsync**

From `koina/safe.ts`:
```typescript
export function trySafe<T>(label: string, fn: () => T, fallback: T): T
export async function trySafeAsync<T>(label: string, fn: () => Promise<T>, fallback: T): Promise<T>
```

Usage:
```typescript
const result = trySafe("non-critical-operation", () => mayFail(), fallbackValue);
```

Logs `warn` level on failure, returns fallback. No exception thrown.

## Logging

**Framework:** `tslog` with structured AsyncLocalStorage context

**Logger creation:**
```typescript
import { createLogger } from "../koina/logger.js";
const log = createLogger("module-name");
```

Module names use colon for sub-modules: `daemon:retention`, `distillation.extract`, `semeion:listen`

**Levels:** silly, trace, debug, info, warn, error, fatal

**Context propagation:**
- Turn context (turnId, sessionId, nousId, etc.) automatically included in all logs via `AsyncLocalStorage`
- Set context with: `withTurn()` or `withTurnAsync()`
- Access context: `getTurnContext()`
- Update context: `updateTurnContext(updates)`

Example:
```typescript
export async function handleMessage(msg: string) {
  return withTurnAsync({ sessionId: "ses_123" }, async () => {
    log.info("Processing message");  // turnId and sessionId auto-included
  });
}
```

**JSON logging:**
- Set env var `ALETHEIA_LOG_JSON=/path/to/logs.jsonl`
- Outputs newline-delimited JSON: `{"ts":"...", "level":"info", "module":"...", "turnId":"...", "msg":"..."}`

**Truncation:**
- Use `sanitizeForLog(value, maxLen)` to truncate long strings before logging
- Prevents PII-heavy payloads from appearing in logs

## Comments

**When to comment:**
- WHY something is done, not WHAT (code should be self-documenting)
- Non-obvious algorithms or complex logic only
- Inline comments only for complex conditionals or counterintuitive behavior

**Bad comments:**
```typescript
// Increment counter
counter++;

// Check if null
if (value === null) { }
```

**Good comments:**
```typescript
// Monotonically increasing turn ID for correlation; resets monthly to prevent overflow
function generateTurnId(): string { ... }

// Close truncated JSON — caller may have streamed incomplete response
if (findBalancedBraces(result!)) { ... }
```

**JSDoc/TSDoc:**
- Minimal use — only for public API functions
- Example from `koina/logger.ts`:
```typescript
/**
 * Run a function with a new turn context. All log calls inside inherit the context.
 */
export function withTurn<T>(ctx: Partial<TurnContext>, fn: () => T): T { ... }
```

**File headers:**
- Single-line comment describing the file's purpose
- No author, date, or timestamp info

```typescript
// Structured error hierarchy for machine-readable error handling
// Data retention enforcement — purge distilled messages, truncate tool results
// Hook system tests
```

## Function Design

**Size:** Prefer small functions. Large files indicate need for extraction.
- Largest file: `mneme/store.ts` (2395 lines) — acceptable because it's an SQLite schema and all schema-related queries
- Distillation pipeline: split into `extract.ts`, `reflect.ts`, `pipeline.ts`, etc.

**Parameters:**
- Use object parameters for functions with 3+ arguments
  ```typescript
  export function runRetention(store: SessionStore, privacy: PrivacySettings): RetentionResult
  ```
- Named parameters are self-documenting

**Return values:**
- Use explicit return types (never `any`)
- Interface-based returns for complex results
  ```typescript
  export interface RetentionResult {
    distilledMessagesDeleted: number;
    archivedMessagesDeleted: number;
    toolResultsTruncated: number;
    ephemeralSessionsDeleted: number;
  }
  ```
- Async functions always return `Promise<T>`

## Module Design

**Exports:**
- Explicit exports, no default exports
- `export function`, `export interface`, `export const`
- Private helpers not exported

**Barrel files:**
- Not commonly used; most imports are direct from module file

**Module structure example:**
```
src/
├── koina/               # Common utilities
│   ├── errors.ts        # AletheiaError hierarchy
│   ├── error-codes.ts   # Error code constants
│   ├── safe.ts          # trySafe/trySafeAsync
│   └── logger.ts        # Logging with AsyncLocalStorage
├── taxis/               # Configuration
├── mneme/               # Memory/session storage
├── nous/                # Core orchestration
└── distillation/        # Extraction/reflection pipeline
```

## Type System

**Strict mode enabled.**

Key strictness settings in `tsconfig.json`:
- `noUnusedLocals: true` — variables must be used
- `noUnusedParameters: true` — parameters must be used
- `exactOptionalPropertyTypes: true` — optional fields can't accept undefined
- `noUncheckedIndexedAccess: true` — bracket notation access typed as `T | undefined`
- `noPropertyAccessFromIndexSignature: true` — only access properties defined explicitly
- `isolatedModules: true` — each file is independent module

**Readonly**
- Use `readonly` on interface properties that shouldn't change
  ```typescript
  export interface AletheiaErrorOpts {
    readonly code: ErrorCode;
    readonly module: string;
  }
  ```

---

*Convention analysis: 2026-02-23*
