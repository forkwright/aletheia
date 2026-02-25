# TypeScript Rules

Agent-action rules for TypeScript in `infrastructure/runtime/src/`.

---

## Error Handling

Throw typed errors only. Never throw bare `Error` or strings.

Compliant:
```typescript
throw new SessionError("session not found", { code: "SESSION_NOT_FOUND", context: { sessionId } });
throw new ToolError("execution failed", { code: "TOOL_EXEC_FAILED" });
```

Non-compliant:
```typescript
throw new Error("session not found");
throw "unexpected state";
```

Use `trySafe`/`trySafeAsync` from `koina/safe.ts` for non-critical operations:
```typescript
const result = trySafe("skill extraction", () => extractSkill(data), null);
const loaded = await trySafeAsync("config load", () => loadConfig(path), defaultConfig);
```

Every catch block must log, rethrow, return a value, or include an explicit `/* reason */` comment.

Non-compliant:
```typescript
try { await processMessage(msg); } catch (e) {}
try { await riskyOp(); } catch (e) { /* TODO */ }
```

See: docs/STANDARDS.md#rule-typed-errors-only
See: docs/STANDARDS.md#rule-no-silent-catch

---

## Logging

Use `createLogger("module-name")` for all daemon logging. Never use `console.*` in daemon context.

Compliant:
```typescript
const log = createLogger("nous:pipeline");
log.info("Turn complete", { sessionId, turnId, tokens });
log.error("Provider error", { err, model });
```

Non-compliant:
```typescript
console.log("Turn complete", sessionId);
console.error("Provider error:", err);
```

`console.*` is acceptable only in CLI output functions explicitly writing to stdout (e.g., `nous/audit.ts`).

See: docs/STANDARDS.md#rule-logger-not-console

---

## No Explicit Any

Use `unknown` with type narrowing. Never annotate with `any`.

Compliant:
```typescript
function process(input: unknown): string {
  if (typeof input !== "string") throw new ValidationError("Expected string");
  return input.toUpperCase();
}
```

Non-compliant:
```typescript
function process(input: any): string {
  return input.toUpperCase();
}
```

When `any` is unavoidable at an external SDK boundary:
```typescript
// oxlint-disable-next-line typescript/no-explicit-any -- SDK returns untyped response
const raw: any = sdk.call();
```

See: docs/STANDARDS.md#rule-no-explicit-any

---

## Async/Await

Synchronous `ToolHandler.execute()` branches must use `return Promise.resolve(result)`, not `async` with no `await`.

Compliant:
```typescript
execute(input: Record<string, unknown>): Promise<string> {
  const result = computeResult(input);
  return Promise.resolve(JSON.stringify(result));
}
```

Non-compliant:
```typescript
async execute(input: Record<string, unknown>): Promise<string> {
  // no await — triggers require-await
  return JSON.stringify(computeResult(input));
}
```

Await every promise or assign to `void` for intentional fire-and-forget:
```typescript
await manager.handleMessage(msg);
void cleanupExpiredSessions(store); // intentional fire-and-forget
```

Never discard a promise silently:
```typescript
manager.handleMessage(msg); // non-compliant — return value discarded
```

See: docs/STANDARDS.md#rule-typed-promise-returns-on-sync-toolhandler-branches
See: docs/STANDARDS.md#rule-no-floating-promises

---

## Import Conventions

Use `.js` extensions on all relative imports. Sort named imports alphabetically within a statement. Use `import type` for type-only imports.

Compliant:
```typescript
import { AletheiaError, SessionError } from "../koina/errors.js";
import { createLogger, type Logger } from "../koina/logger.js";
import type { AletheiaConfig } from "./taxis/schema.js";
```

Non-compliant:
```typescript
import { createLogger } from "../koina/logger";          // missing .js
import { type Logger, createLogger } from "../koina/logger.js"; // not sorted
import { AletheiaConfig } from "./taxis/schema.js";      // type-only, not import type
```

Import order: node built-ins → external packages → internal modules → local files.

See: docs/STANDARDS.md#rule-js-import-extensions
See: docs/STANDARDS.md#rule-sort-named-imports-within-statement
See: docs/STANDARDS.md#rule-type-only-imports-with-import-type

---

## Naming

- Files: `kebab-case.ts`
- Classes: `PascalCase`
- Functions: `camelCase`, verb-first (e.g., `loadConfig`, `createSession`)
- Constants: `UPPER_SNAKE`
- Events: `noun:verb` (e.g., `turn:before`, `tool:called`)

---

## Testing

Write one logical assertion per test. Name tests descriptively.

Compliant:
```typescript
it("returns empty array when session has no turns", async () => {
  const result = await store.getTurns(emptySessionId);
  expect(result).toEqual([]);
});
```

Non-compliant:
```typescript
it("works", async () => {
  // multiple unrelated assertions
  expect(a).toBe(1);
  expect(b).toBe(2);
});
```

Place test files in the same directory as the source: `my-module.test.ts`. Test behavior, not implementation.
