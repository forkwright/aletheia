# Spec: Code Quality — Error Handling, Dead Code Audit, and Coding Standards

**Status:** Phase 1-4 done (PRs #37, #45)  
**Author:** Syn  
**Date:** 2026-02-19  

---

## Problem

### Error Handling

The codebase has inconsistent error handling patterns. Some errors are caught and logged, some are caught and silently swallowed, some bubble up unhandled. The turn-safety spec (01) addresses the pipeline specifically, but the problem is systemic:

- **Try/catch with empty catch blocks** — errors vanish without a trace
- **`catch(() => {})` on promise chains** — prevents unhandled rejections but loses all context
- **Inconsistent error types** — some modules throw strings, some throw Error, some throw custom types, some return null
- **Missing error boundaries** — a failure in a non-critical subsystem (skill learning, interaction signals, competence tracking) can crash the turn
- **No structured error taxonomy** — errors don't carry codes, categories, or recoverability flags (except in `hermeneus/anthropic.ts` which does this correctly with `ProviderError`)

### Dead Code

The codebase has accumulated features, endpoints, and utilities that were written speculatively or superseded by later work. These add maintenance burden, confuse contributors, and bloat the build. Examples observed during spec research:

- Endpoints that exist but aren't called by any client
- Utility functions imported in zero files
- Config schema fields that no code reads
- Entire modules that were scaffolded but never integrated (parts of `auth/` are wired, parts aren't)
- CSS that styles components that no longer exist
- Test helpers that test removed functionality

### Coding Standards

There's no documented agreement on how code should be written. The result is inconsistency across modules — some files are heavily commented, some have zero comments, some use different patterns for the same problem. Establishing standards isn't about bureaucracy; it's about reducing cognitive load when reading code across the codebase.

---

## Part 1: Error Handling Overhaul

### Error Taxonomy

Adopt the `ProviderError` pattern from `hermeneus/anthropic.ts` as the base for all error types:

```typescript
// koina/errors.ts — centralized error types

export interface ErrorContext {
  code: string;
  recoverable: boolean;
  retryAfterMs?: number;
  context?: Record<string, unknown>;
  cause?: unknown;
}

export class AletheiaError extends Error {
  readonly code: string;
  readonly recoverable: boolean;
  readonly retryAfterMs?: number;
  readonly context: Record<string, unknown>;

  constructor(message: string, opts: ErrorContext) {
    super(message, { cause: opts.cause });
    this.name = "AletheiaError";
    this.code = opts.code;
    this.recoverable = opts.recoverable;
    this.retryAfterMs = opts.retryAfterMs;
    this.context = opts.context ?? {};
  }
}

// Specific subclasses for major categories
export class PipelineError extends AletheiaError { name = "PipelineError"; }
export class StoreError extends AletheiaError { name = "StoreError"; }
export class ToolError extends AletheiaError { name = "ToolError"; }
export class TransportError extends AletheiaError { name = "TransportError"; }
export class ConfigError extends AletheiaError { name = "ConfigError"; }
```

**Error codes** follow a namespaced pattern: `PIPELINE_STAGE_FAILED`, `STORE_WRITE_FAILED`, `TOOL_TIMEOUT`, `TRANSPORT_SEND_FAILED`, `CONFIG_VALIDATION_FAILED`. These are greppable, countable, and alertable.

### Audit Scope

Review every file in `infrastructure/runtime/src/` for:

| Pattern | Action |
|---------|--------|
| `catch(() => {})` or `catch { }` | Log with context, or remove the catch and let it propagate |
| `catch(err) { /* no log */ }` | Add `log.warn` or `log.error` with the error and context |
| `throw "string"` | Replace with `throw new AletheiaError(...)` |
| Return `null` on error without logging | Add structured log before return |
| Non-critical subsystems without isolation | Wrap in try/catch with warning-level log, don't crash the caller |

### Error Boundary Pattern

Non-critical operations (skill learning, interaction signals, competence tracking, workspace flush) should use a consistent boundary:

```typescript
function trySafe<T>(label: string, fn: () => T, fallback: T): T {
  try {
    return fn();
  } catch (err) {
    log.warn(`${label} failed (non-fatal): ${err instanceof Error ? err.message : err}`);
    return fallback;
  }
}

// Usage:
trySafe("skill extraction", () => extractSkillCandidate(...), null);
```

For async operations:

```typescript
async function trySafeAsync<T>(label: string, fn: () => Promise<T>, fallback: T): Promise<T> {
  try {
    return await fn();
  } catch (err) {
    log.warn(`${label} failed (non-fatal): ${err instanceof Error ? err.message : err}`);
    return fallback;
  }
}
```

These make the intent explicit: "this operation is optional and must not crash the caller."

---

## Part 2: Dead Code Audit

### Automated Detection

Run `knip` (already in nightly workflow) and `ts-prune` to detect:

- **Unused exports** — functions/types exported but imported nowhere
- **Unused files** — files not imported by any other file
- **Unused dependencies** — npm packages in `package.json` but not imported
- **Unused config fields** — schema defines fields that no runtime code reads

### Manual Audit Checklist

| Area | What to check |
|------|---------------|
| **API endpoints** | For every `app.get/post/put/delete` in `server.ts`, verify at least one client calls it. Remove or document as "internal only" |
| **Auth modules** | `auth/audit.ts`, `auth/rbac.ts`, `auth/sessions.ts` — which are wired into the gateway vs. dormant? Document state in code |
| **Schema fields** | For every field in `schema.ts`, grep for its usage. Remove fields no code reads |
| **CSS** | For every `.class` in Svelte styles, verify the class is used in the template. Remove orphaned styles |
| **Test helpers** | Remove tests that test functionality that no longer exists |
| **Shared/bin scripts** | For every script in `shared/bin/`, verify it's still relevant and documented |
| **Skills** | `shared/skills/` — are these used by any agent? Or accumulated automatically and never invoked? |

### Deliverable

A single PR that:
1. Removes all confirmed dead code
2. Adds `// TODO(unused): reason` comments on code that's scaffolded-but-not-yet-integrated (like parts of auth) so it doesn't get deleted in future sweeps
3. Updates the knip config to track the reduced baseline

---

## Part 3: Coding Standards

### Self-Documenting Code Over Comments

**Header comments:** Each file gets exactly one header comment — a single line or brief block explaining what the module is and its role in the system:

```typescript
// Pipeline runner — composes stages for streaming and non-streaming turn execution
```

This is already done well in most files. Standardize the format: `// Module purpose — brief description`.

**Inline comments:** Only where the *why* is non-obvious. Never comment *what* the code does — the code should say that itself. Good:

```typescript
// SQLCipher 4 format — must be set before any other pragma
this.db.pragma(`key = '${encryptionKey}'`);
```

Bad:

```typescript
// Get the session
const session = store.findSessionById(sessionId);
```

**Delete all "what" comments.** If the code needs a "what" comment to be understood, rename the variables and functions until it doesn't.

### Naming Conventions

| Thing | Convention | Example |
|-------|-----------|---------|
| Files | kebab-case | `build-messages.ts`, `session-store.ts` |
| Classes | PascalCase | `SessionStore`, `ToolRegistry` |
| Functions | camelCase, verb-first | `resolveThread`, `buildContext`, `parseConfig` |
| Constants | UPPER_SNAKE | `MAX_CONCURRENT_TURNS`, `SIDECAR_URL` |
| Types/Interfaces | PascalCase | `TurnState`, `DistillationOpts` |
| Boolean vars | `is`/`has`/`should` prefix | `isStreaming`, `hasToken`, `shouldDistill` |
| Event names | `noun:verb` or `noun:adjective` | `turn:before`, `distill:after`, `tool:called` |

### Function Design

- **Single responsibility.** If a function needs a comment explaining its sections, split it.
- **Early returns over nested ifs.** Guard clauses at the top, happy path below.
- **Explicit over clever.** The next person reading this code is a tired engineer at 2am. Write for them.
- **Right tool for the job.** Before using a pattern, ask: "Is this the simplest correct solution?" Not "does this work?" but "is this the right way?" Examples:
  - Don't use `reduce` when a `for` loop is clearer
  - Don't use a class when a function suffices
  - Don't use a Map when a plain object works
  - Don't use generics when the type is always the same

### Error Handling Standards

- **Never empty catch.** Every catch either logs, rethrows, or returns a meaningful value.
- **Use typed errors.** `throw new PipelineError(...)` not `throw new Error(...)`.
- **Non-critical = explicit boundary.** Use `trySafe`/`trySafeAsync` for optional operations.
- **Log at the boundary.** The function that catches the error logs it. Inner functions let errors propagate.

### Import Organization

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

### Testing Standards

- **Test behavior, not implementation.** Tests should break when the contract changes, not when internals refactor.
- **One assertion per test** (where practical). Name describes the assertion.
- **Test names:** `it("returns null when session not found")` not `it("test 1")`.
- **No internal state access in tests.** If you need `(store as any).db.prepare(...)` in a test, the store needs a method for that.

### Enforcement

- **ESLint rules** for no-empty-catch, consistent-type-imports, import ordering
- **Pre-commit hook** runs lint
- **PR review** checks for compliance — Claude Code should be configured with these standards in its CLAUDE.md

---

## Implementation Order

| Phase | Effort | Impact |
|-------|--------|--------|
| **1: Standards doc** — write `CONTRIBUTING.md` at repo root with the above | Small | Sets expectations |
| **2: Error taxonomy** — create `koina/errors.ts`, `koina/safe.ts` | Small | Foundation for cleanup |
| **3: Automated audit** — run knip, ts-prune, document findings | Small | Identifies scope |
| **4: Dead code removal** — single PR removing confirmed dead code | Medium | Reduces surface area |
| **5: Error handling sweep** — file-by-file audit of catch blocks | Medium | Reliability |
| **6: Comment cleanup** — remove "what" comments, standardize headers | Small | Readability |
| **7: ESLint rules** — encode standards in tooling | Small | Automated enforcement |

---

## Success Criteria

- **Zero empty catch blocks** in the codebase
- **Every error** has a code, a log, and a recoverability flag
- **knip reports zero** unused exports (or all exceptions are documented)
- **Every file** has exactly one header comment and zero "what" comments
- **`CONTRIBUTING.md`** exists and Claude Code's CLAUDE.md references it
