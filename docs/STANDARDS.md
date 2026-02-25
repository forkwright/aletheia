# Aletheia Code Standards

> The fixed target for v1.1 tooling, auditing, and remediation.
> Each rule: what / why / compliant / non-compliant / enforced-by / scan count.
> Last updated: 2026-02-25

---

## TypeScript

### Rule: Typed Errors Only

**What:** All thrown errors must extend `AletheiaError`. Never `throw new Error(...)` or throw strings. Non-critical operations use `trySafe`/`trySafeAsync` from `koina/safe.ts`.

**Why:** Bare errors are uncatchable by type — callers cannot distinguish error categories. The typed hierarchy enables targeted handling, structured logging, and clean error propagation across the call stack.

**Compliant:**
```typescript
throw new SessionError("session not found", { code: "SESSION_NOT_FOUND", context: { sessionId } });

// Non-critical path:
const result = trySafe("skill extraction", () => extractSkill(data), null);
```

**Non-compliant:**
```typescript
throw new Error("session not found");
throw "unexpected state";
```

**Enforced by:** Convention + agent context (oxlint has no rule for this pattern; Phase 12 manual audit covers it). See `koina/errors.ts` for the full hierarchy and `koina/error-codes.ts` for codes.

**Scan count:** 41 bare `throw new Error(` in non-test runtime source (2026-02-25). 31 typed `AletheiaError` subclass throws in same scope. The ratio confirms adoption is partial — remediation is Phase 13.

---

### Rule: No Silent Catch

**What:** Every catch block must either log the error, rethrow it, return a meaningful value, or include an explicit `/* reason */` comment explaining why the error is intentionally discarded.

**Why:** Silent catch blocks hide failures silently. In a daemon process there is no interactive feedback — a swallowed error is an invisible bug.

**Compliant:**
```typescript
try {
  await processMessage(msg);
} catch (err) {
  log.error("Message processing failed", { err });
  throw err;
}

// Intentional discard with explanation:
try {
  chmodSync(path, 0o600);
} catch { /* non-fatal: file may already have correct permissions */ }
```

**Non-compliant:**
```typescript
try {
  await processMessage(msg);
} catch (e) {}

try {
  await riskyOp();
} catch (e) { /* TODO */ }
```

**Enforced by:** `no-empty` (oxlint, currently `error`) catches empty catch blocks. Silent catch with a body (e.g., a comment-only block) is convention + agent context.

**Scan count:** 0 empty catch blocks detected by `no-empty` rule. Convention violations (catch blocks with only a TODO or unused variable) are not mechanically counted — Phase 12 audit will surface them.

---

### Rule: No Explicit Any

**What:** Do not use `any` as a type annotation. Use `unknown` with type narrowing, or define a proper interface/type.

**Why:** `any` disables TypeScript's type checker for the annotated value and everything that flows through it. In a typed codebase it creates invisible holes in the type system.

**Compliant:**
```typescript
function process(input: unknown): string {
  if (typeof input !== "string") throw new ValidationError("Expected string");
  return input.toUpperCase();
}
```

**Non-compliant:**
```typescript
function process(input: any): string {
  return input.toUpperCase(); // no type check
}
```

**Enforced by:** `typescript/no-explicit-any` (oxlint, currently `warn` — deferred to Phase 14, 32 violations).

**Scan count:** 32 violations in non-test runtime source (2026-02-25).

---

### Rule: Logger Not Console

**What:** Use `createLogger("module-name")` for all daemon logging. `console.*` is acceptable only in CLI-mode functions that explicitly produce human-readable stdout output (e.g., `nous/audit.ts`).

**Why:** `createLogger` includes structured context (session ID, turn ID, agent ID) via AsyncLocalStorage. `console.*` in daemon code loses all context correlation and produces unstructured output that cannot be filtered, routed, or aggregated.

**Compliant:**
```typescript
const log = createLogger("nous:pipeline");

log.info("Turn complete", { sessionId, turnId, tokens });
log.error("Provider error", { err, model });

// Legitimate CLI stdout (in a CLI output function like audit.ts):
console.log(formatTable(auditResults));
```

**Non-compliant:**
```typescript
// In daemon code:
console.log("Turn complete", sessionId);
console.error("Provider error:", err);
```

**Enforced by:** `no-console` (oxlint, Phase 11 addition — not yet in `.oxlintrc.json`). Exception: `nous/audit.ts` and any function explicitly documented as producing CLI stdout output.

**Scan count:** 271 total `console.*` in non-test runtime source. ~60 are in `nous/audit.ts` (legitimate CLI output). 256 excluding `audit.ts` — all are candidates for `createLogger` migration in Phase 13.

---

### Rule: Typed Promise Returns on Sync ToolHandler Branches

**What:** `ToolHandler.execute()` implementations that are synchronous in some branches must use `return Promise.resolve(result)` rather than the `async` keyword without any `await`.

**Why:** The `async` keyword on a function with no `await` triggers `eslint(require-await)`. The structural fix is to preserve the `Promise<string>` return type without the `async` keyword. Do not remove `async` and change the return type — that breaks the `ToolHandler` interface contract.

**Compliant:**
```typescript
export const myTool: ToolHandler = {
  definition: { ... },
  execute(input: Record<string, unknown>): Promise<string> {
    const result = computeResult(input);
    return Promise.resolve(JSON.stringify(result));
  },
};
```

**Non-compliant:**
```typescript
export const myTool: ToolHandler = {
  definition: { ... },
  async execute(input: Record<string, unknown>): Promise<string> {
    // no await — triggers require-await
    const result = computeResult(input);
    return JSON.stringify(result);
  },
};
```

**Enforced by:** `require-await` (oxlint, currently `warn` — deferred to Phase 14, 125 violations). See CONTRIBUTING.md Gotcha 3.

**Scan count:** 125 violations, concentrated in `organon/built-in/*.ts` (the ToolHandler pattern). Non-organon violations are genuine bugs — 0 found outside organon in current scan.

---

### Rule: Sort Named Imports Within Statement

**What:** Named imports within a single `import { }` statement must be sorted alphabetically (case-insensitive).

**Why:** Consistent import ordering reduces diff noise and makes it easier to scan whether a specific name is imported from a module.

**Compliant:**
```typescript
import { createLogger, type Logger } from "../koina/logger.js";
import { AletheiaError, PlanningError, SessionError } from "../koina/errors.js";
```

**Non-compliant:**
```typescript
import { type Logger, createLogger } from "../koina/logger.js";
import { SessionError, AletheiaError, PlanningError } from "../koina/errors.js";
```

**Enforced by:** `sort-imports` (oxlint, currently `warn` — deferred to Phase 14, 68 violations). Note: `ignoreDeclarationSort: true` means statement-level ordering is not enforced — only member-level sorting within a single import statement.

**Scan count:** 68 violations in runtime source (2026-02-25).

---

### Rule: .js Import Extensions

**What:** All relative imports must include the `.js` extension, even for `.ts` source files.

**Why:** TypeScript with `"moduleResolution": "bundler"` and ESM output requires `.js` extensions for Node.js compatibility. Omitting them causes runtime module resolution failures in the built output.

**Compliant:**
```typescript
import { createLogger } from "../koina/logger.js";
import type { SessionStore } from "../mneme/store.js";
```

**Non-compliant:**
```typescript
import { createLogger } from "../koina/logger";
import type { SessionStore } from "../mneme/store";
```

**Enforced by:** `import/extensions` (oxlint — verify rule name in Phase 11; currently enforced via tsconfig `moduleResolution` which fails to resolve extensionless imports at build time).

**Scan count:** 0 — build fails on extensionless imports. Already universally compliant.

---

### Rule: Type-Only Imports with `import type`

**What:** Import type-only symbols using `import type { }` syntax, not `import { }`. When mixing value and type imports from the same module, use inline `type` modifier: `import { value, type MyType }`.

**Why:** `import type` is erased at compile time — it produces no runtime module load. Mixing runtime and type imports creates unnecessary module dependencies and increases bundle size.

**Compliant:**
```typescript
import type { AletheiaConfig } from "./taxis/schema.js";
import { createLogger, type Logger } from "../koina/logger.js";
```

**Non-compliant:**
```typescript
import { AletheiaConfig } from "./taxis/schema.js"; // type used as value import
```

**Enforced by:** `typescript/consistent-type-imports` (oxlint, currently `error`). Already enforced.

**Scan count:** 1 `import/no-duplicates` error — a duplicate import from the same module, which this rule also covers. 0 `consistent-type-imports` violations (rule is at `error` level and already passing).

---

### Rule: No Floating Promises

**What:** Every `Promise` returned by an `async` function must be `await`ed, returned, or explicitly handled. Do not fire-and-forget unless the intent is documented.

**Why:** Unhandled promise rejections crash the process in Node.js 22+. Fire-and-forget patterns lose error context and make debugging impossible.

**Compliant:**
```typescript
await manager.handleMessage(msg);

// Intentional fire-and-forget with void:
void cleanupExpiredSessions(store);
```

**Non-compliant:**
```typescript
manager.handleMessage(msg); // return value discarded
processQueue(); // promise rejection silently dropped
```

**Enforced by:** `typescript/no-floating-promises` (oxlint, currently `error`). Already enforced.

**Scan count:** 0 violations — rule is at `error` level and already passing.

---

## Svelte

### Rule: No XSS via @html

**What:** Do not use `{@html ...}` with user-supplied or externally-sourced content unless the content has been sanitized through a verified sanitization library.

**Why:** `{@html}` bypasses Svelte's automatic HTML escaping and renders raw HTML directly into the DOM. Unsanitized user content creates a cross-site scripting (XSS) vulnerability.

**Compliant:**
```svelte
<!-- Static, developer-controlled content -->
{@html marked(staticMarkdown)}

<!-- User content — sanitize first -->
{@html DOMPurify.sanitize(userInput)}
```

**Non-compliant:**
```svelte
{@html userMessage}
{@html apiResponse.content}
```

**Enforced by:** `svelte/no-at-html-tags` (eslint-plugin-svelte, Phase 11 addition). Currently enforced by convention + agent context.

**Scan count:** Not scanned — UI source not covered by current oxlint config. Phase 12 Svelte audit will establish baseline.

---

### Rule: Svelte 5 Runes Only (no legacy reactive syntax)

**What:** Use Svelte 5 rune syntax (`$state`, `$derived`, `$effect`, `$props`) exclusively. Do not use legacy reactive declarations (`$:`, `export let`, reactive stores with `$storeName` auto-subscription syntax in script blocks).

**Why:** Aletheia targets Svelte 5. Legacy reactive syntax is deprecated in Svelte 5 and will be removed. Mixing syntaxes creates ambiguous component behavior and blocks future Svelte upgrades.

**Compliant:**
```svelte
<script lang="ts">
  let count = $state(0);
  let doubled = $derived(count * 2);
  let { label } = $props<{ label: string }>();

  $effect(() => {
    console.log("count changed:", count);
  });
</script>
```

**Non-compliant:**
```svelte
<script lang="ts">
  export let label: string;     // legacy prop
  let count = 0;
  $: doubled = count * 2;       // legacy reactive declaration
</script>
```

**Enforced by:** Convention + agent context. Svelte 5 compiler warns on legacy syntax in runes mode. `svelte-check` escalates warnings to errors (see Rule: svelte-check Warnings Are Errors).

**Scan count:** Not scanned — UI lint coverage added in Phase 11. Baseline established then.

---

### Rule: svelte-check Warnings Are Errors

**What:** `svelte-check` must pass with zero warnings in CI. Warnings are treated as errors for gating purposes.

**Why:** `svelte-check` catches type errors, missing props, and deprecated API usage in Svelte components. Allowing warnings to accumulate creates silent technical debt that is expensive to remediate later.

**Compliant:**
All props typed, no deprecated APIs, `svelte-check` exits 0.

**Non-compliant:**
Any `svelte-check` warning left unaddressed; `svelte-check` run without `--fail-on-warnings`.

**Enforced by:** CI step `cd ui && npx svelte-check --fail-on-warnings` (Phase 11 addition).

**Scan count:** Not scanned — UI CI integration added in Phase 11.

---

### Rule: Typed Component Props

**What:** All Svelte component props must be explicitly typed using `$props<{ ... }>()` with a TypeScript interface or inline type. No untyped or `any`-typed props.

**Why:** Untyped props break type-checking at component boundaries. TypeScript cannot verify that parent components pass the correct prop types.

**Compliant:**
```svelte
<script lang="ts">
  interface Props {
    agentId: string;
    isLoading?: boolean;
    onSubmit: (message: string) => void;
  }
  let { agentId, isLoading = false, onSubmit } = $props<Props>();
</script>
```

**Non-compliant:**
```svelte
<script lang="ts">
  let { agentId, isLoading, onSubmit } = $props(); // no type parameter
</script>
```

**Enforced by:** `svelte-check` (type checking at component boundaries) + convention + agent context.

**Scan count:** Not scanned — UI coverage added in Phase 11.

---

## Python (Memory Sidecar)

### Rule: FastAPI Depends() Pattern (not B008)

**What:** Use `fastapi.Depends()` for dependency injection in FastAPI route signatures. Do not use mutable default arguments or call functions directly in parameter defaults.

**Why:** FastAPI's `Depends()` mechanism handles lifecycle, caching, and async context correctly. Calling functions directly in default arguments (ruff rule B008) executes them at module import time rather than per-request.

**Compliant:**
```python
from fastapi import Depends

def get_db() -> Database:
    return Database(settings.db_url)

@app.post("/search")
async def search(query: SearchQuery, db: Database = Depends(get_db)):
    return await db.search(query.text)
```

**Non-compliant:**
```python
@app.post("/search")
async def search(query: SearchQuery, db: Database = Database(settings.db_url)):  # B008
    return await db.search(query.text)
```

**Enforced by:** ruff rule `B008` (Phase 11 addition to sidecar CI). FastAPI pattern is also validated by `pyright` strict mode.

**Scan count:** Not scanned — ruff not yet configured for the sidecar (pyproject.toml has no `[tool.ruff]` section as of 2026-02-25). Phase 11 establishes the baseline.

---

### Rule: Ruff-Selected Rule Set

**What:** The sidecar must pass ruff lint with the following rule sets enabled: `E`, `W`, `F`, `B`, `I`, `UP`. No `# noqa` suppression without an inline comment explaining why.

**Why:** The sidecar currently has zero static analysis configured. `pyproject.toml` has no `[tool.ruff]` section. This leaves the Python codebase entirely unchecked. The selected rule sets cover pyflakes errors (`F`), pycodestyle (`E`/`W`), bugbear patterns (`B`), import ordering (`I`), and pyupgrade modernization (`UP`).

**Compliant:**
```python
# pyproject.toml:
[tool.ruff]
select = ["E", "W", "F", "B", "I", "UP"]
ignore = ["B008"]  # FastAPI Depends() — intentional, see STANDARDS.md
```

**Non-compliant:**
```python
# pyproject.toml has no [tool.ruff] section — no lint enforcement
```

**Enforced by:** ruff (Phase 11 addition to `pyproject.toml` and CI).

**Scan count:** 0 — no ruff config exists yet. Baseline established in Phase 11.

---

### Rule: Pyright Strict Mode

**What:** The sidecar must pass `pyright --strict` with zero errors. All functions must have explicit return type annotations. All parameters must be typed.

**Why:** FastAPI route handlers with untyped parameters produce incorrect OpenAPI schemas and miss runtime validation errors. Pyright strict mode catches these at development time rather than in production.

**Compliant:**
```python
# pyproject.toml:
[tool.pyright]
strict = ["aletheia_memory/**"]

# Route handler:
async def add_memories(request: AddMemoriesRequest) -> AddMemoriesResponse:
    ...
```

**Non-compliant:**
```python
async def add_memories(request):  # untyped — pyright strict error
    ...
```

**Enforced by:** pyright (Phase 11 addition to CI). Not yet configured.

**Scan count:** 0 — pyright not yet configured. Baseline established in Phase 11.

---

### Rule: No Bare Exception Catch

**What:** Do not use bare `except:` or `except Exception:` without re-raising or logging the specific error with context. Always catch the most specific exception type available.

**Why:** Bare `except:` catches `SystemExit`, `KeyboardInterrupt`, and other non-error signals. Even `except Exception:` swallows errors silently if not properly handled. FastAPI routes with silent exception swallowing return 500 errors with no diagnostic information.

**Compliant:**
```python
try:
    result = await mem0.search(query)
except MemorySearchError as e:
    logger.error("Memory search failed", query=query, error=str(e))
    raise HTTPException(status_code=503, detail="Memory search unavailable") from e
```

**Non-compliant:**
```python
try:
    result = await mem0.search(query)
except:  # bare except
    return []

try:
    result = await mem0.search(query)
except Exception:  # swallowed
    pass
```

**Enforced by:** ruff rule `BLE001` (blind exception catch) and `E722` (bare except). Phase 11 addition.

**Scan count:** Not scanned — ruff not yet configured. Phase 11 establishes baseline.

---

## Naming

### Rule: Gnomon Naming Convention

**What:** Persistent names for modules, subsystems, agents, and major components must follow the naming system documented in [gnomon.md](gnomon.md). Names identify modes of attention, pass the layer test (L1-L4), and compose with the existing name topology.

**Why:** The naming system is not decoration. Names that identify the right mode of attention survive refactors, communicate architectural intent, and resist the drift toward generic labels. A well-chosen name teaches you something about the thing it names. See gnomon.md for the full philosophy, method, and anti-patterns.

**Applies to:** Module directories, agent identities, subsystem names, major features that will persist. Does *not* apply to: utility functions, variable names, temporary branches, test fixtures.

**Process:**
1. Identify the mode of attention (not the implementation)
2. Construct from Greek roots using prefix-root-suffix system
3. Run the layer test (L1 practical through L4 reflexive)
4. Check topology against existing names
5. If no Greek word fits naturally, the mode of attention isn't clear yet - wait

**Enforced by:** Convention + agent context. Agents should have gnomon.md in context when proposing names for new persistent components.

---

## Architecture

### Rule: Module Import Direction (layered graph)

**What:** Imports must flow from higher layers to lower layers only. See `docs/ARCHITECTURE.md` for the full dependency table with each module's permitted and forbidden imports.

**Why:** Circular imports cause initialization order bugs and create tight coupling between modules that should be independent. Node.js ESM does not support circular dependencies cleanly — they produce `undefined` values at import time.

**Compliant:**
```typescript
// In nous/ (higher layer): may import from organon, hermeneus, taxis, koina
import { ToolRegistry } from "../organon/registry.js";
import { createDefaultRouter } from "../hermeneus/router.js";
```

**Non-compliant:**
```typescript
// In koina/ (lowest layer): must not import from any other module
import { loadConfig } from "../taxis/loader.js"; // forbidden — koina is a leaf node
```

**Enforced by:** `import/no-cycle` (oxlint, Phase 11 addition). Convention + agent context for directional enforcement. See `docs/ARCHITECTURE.md#dependency-rules`.

**Scan count:** Circular dependency baseline established in Phase 12. Current count unknown — oxlint `import/no-cycle` not yet configured.

---

### Rule: Event Name Format noun:verb

**What:** All event bus event names must follow `noun:verb` format (e.g., `turn:before`, `tool:called`, `session:created`). No other formats.

**Why:** Consistent `noun:verb` naming makes event subscriptions greppable and predictable. Mixed formats (camelCase, hyphenated, colon-less) make it impossible to find all events related to a subsystem.

**Compliant:**
```typescript
eventBus.emit("turn:before", { sessionId, turnId });
eventBus.emit("tool:called", { name, sessionId });
eventBus.emit("session:created", { id, agentId });
```

**Non-compliant:**
```typescript
eventBus.emit("turnBefore", { sessionId });       // camelCase
eventBus.emit("tool-called", { name });            // hyphenated
eventBus.emit("sessionCreated", { id });           // camelCase, no colon
```

**Enforced by:** Convention + agent context. No mechanical enforcement exists. Phase 12 audit will scan for non-conforming event names.

**Scan count:** Not counted. All events authored during the main build phases follow the pattern; compliance is high but unverified.

---

### Rule: Logger Creation Pattern createLogger("module-name")

**What:** Create loggers at module scope using `createLogger("module-name")`. The module name must be a string literal matching the module's directory name or `"module:subcomponent"` for sub-components.

**Why:** The module name is included in every log line and used for log filtering. Inconsistent naming makes log correlation across modules impossible.

**Compliant:**
```typescript
const log = createLogger("nous:pipeline");
const log = createLogger("dianoia:orchestrator");
const log = createLogger("hermeneus");
```

**Non-compliant:**
```typescript
const log = createLogger("myLogger");           // non-descriptive
const log = createLogger("src/nous/pipeline");  // path format, not module name
createLogger("temp");                           // unnamed, not assigned
```

**Enforced by:** Convention + agent context. `createLogger` is the only way to get a structured logger — no alternative exists in the codebase.

**Scan count:** Not counted. `createLogger` calls are universal in compliant modules; violations would be `console.*` usage (see Rule: Logger Not Console — 271 count).

---

### Rule: Module Boundary Discipline

**What:** Cross-module imports use tsconfig path aliases (`@module`) and barrel exports (`index.ts`). Each module declares its public surface via `index.ts` — consumers import from the barrel, not internal files. `koina/` is the sole exception: it is a leaf node with direct-import discipline (no `index.ts`) to avoid loading the entire commons when only one utility is needed.

**Why:** Module identity should be defined once and referenced everywhere. Direct imports into module internals (e.g., `../symbolon/middleware.js`) couple consumers to internal file structure. Barrel exports + path aliases make the module contract explicit and renames trivial (change tsconfig + directory name, done).

**Compliant:**
```typescript
// Cross-module: use path alias and barrel
import { authMiddleware } from "@symbolon";
import { distillSession } from "@melete";

// koina exception: direct imports (leaf node, no barrel)
import { createLogger } from "../koina/logger.js";

// Within a module: relative imports to internal files
import { hashPassword } from "./passwords.js";
```

**Non-compliant:**
```typescript
// Reaching into module internals from outside:
import { authMiddleware } from "../symbolon/middleware.js";

// koina barrel (would load everything):
import { createLogger } from "../koina/index.js";
```

**Enforced by:** Convention + agent context. Spec 33 Phase 1 will add tsconfig path aliases and barrel exports for all gnomon modules. `eslint-plugin-import` `no-internal-modules` rule planned as mechanical enforcement.

**Scan count:** Not yet enforced mechanically. Current codebase uses direct cross-module imports — migration to barrel + alias pattern is tracked in Spec 33.

**Note:** This rule supersedes the previous "No Barrel Files" rule. The original concern (loading entire modules when only one export is needed) is valid for `koina/` but not for domain modules with a clear public surface. The tsconfig path alias + selective barrel pattern avoids the loading problem while providing the boundary discipline the codebase needs.

---

### Rule: One Module Per PR for Remediation Batches

**What:** When remediating existing violations (Phase 13), each PR must touch at most one module directory. Do not batch multi-module remediations into a single PR.

**Why:** Multi-module PRs are difficult to review, harder to revert if a remediation introduces a regression, and obscure the per-module violation delta in the audit trail.

**Compliant:**
```
PR: "fix: typed errors in organon built-in tools"
  src/organon/built-in/exec.ts
  src/organon/built-in/read.ts
  src/organon/built-in/grep.ts
```

**Non-compliant:**
```
PR: "fix: typed errors across runtime"
  src/organon/built-in/exec.ts
  src/nous/pipeline.ts         ← different module
  src/hermeneus/router.ts      ← different module
```

**Enforced by:** PR review convention. Phase 13 execution plan will batch work by module.

**Scan count:** N/A — process rule, not a code pattern.

---

## Enforcement Summary Table

| Rule | Tool | Config Location | Status |
|------|------|-----------------|--------|
| Typed Errors Only | Convention + agent context | — | Partial (41 violations) |
| No Silent Catch | `no-empty` (oxlint) | `.oxlintrc.json` | Active (error) |
| No Explicit Any | `typescript/no-explicit-any` (oxlint) | `.oxlintrc.json` | Warn → Error in Phase 11 |
| Logger Not Console | `no-console` (oxlint) | Phase 11 addition | Not yet active |
| Typed Promise Returns | `require-await` (oxlint) | `.oxlintrc.json` | Warn (125 violations — Phase 14) |
| Sort Named Imports | `sort-imports` (oxlint) | `.oxlintrc.json` | Warn (68 violations — Phase 14) |
| Prefer await over .then() | `promise/prefer-await-to-then` (oxlint) | `.oxlintrc.json` | Warn (61 violations — Phase 14) |
| Catch param naming | `unicorn/catch-error-name` (oxlint) | `.oxlintrc.json` | Warn (221 violations — Phase 14) |
| .js Import Extensions | tsconfig + build | `tsconfig.json` | Active (build fails) |
| Type-Only Imports | `typescript/consistent-type-imports` (oxlint) | `.oxlintrc.json` | Active (error) |
| No Floating Promises | `typescript/no-floating-promises` (oxlint) | `.oxlintrc.json` | Active (error) |
| No XSS via @html | `svelte/no-at-html-tags` (eslint-plugin-svelte) | Phase 11 addition | Not yet active |
| Svelte 5 Runes Only | Convention + svelte-check | Phase 11 CI | Not yet active |
| svelte-check Warnings | CI step `--fail-on-warnings` | Phase 11 CI | Not yet active |
| Typed Component Props | svelte-check + convention | Phase 11 | Not yet active |
| FastAPI Depends() | ruff `B008` | Phase 11 addition | Not yet active |
| Ruff Rule Set | ruff | Phase 11 addition to pyproject.toml | Not yet active |
| Pyright Strict Mode | pyright | Phase 11 addition | Not yet active |
| No Bare Exception Catch | ruff `BLE001`, `E722` | Phase 11 addition | Not yet active |
| Gnomon Naming Convention | Convention + agent context | [gnomon.md](gnomon.md) | Active |
| Module Import Direction | `import/no-cycle` (oxlint) | Phase 11 addition | Not yet active |
| Event Name Format | Convention + agent context | — | High compliance, unverified |
| Logger Creation Pattern | Convention + agent context | — | Universally followed |
| Module Boundary Discipline | Convention → Spec 33 | `tsconfig.json` (planned) | Migration pending |
| One Module Per PR | PR review convention | — | Process rule |

---

## Pre-commit Hook

The `.githooks/pre-commit` hook runs lint and type checks for each sub-project when relevant files are staged. The full test suite runs in CI only.

### What runs

| Sub-project | Trigger | Commands |
|-------------|---------|----------|
| TypeScript runtime | Any `infrastructure/runtime/` file staged | `npm run typecheck && npm run lint:check` |
| Svelte UI | Any `ui/` file staged | `npm run lint:check` (oxlint + eslint-plugin-svelte) |
| Python sidecar | Any `infrastructure/memory/sidecar/` file staged | `uv run ruff check . && uv run pyright` |

### Known gap

UI `svelte-check --fail-on-warnings` is NOT gated in the hook. The pre-existing 112 errors from before Phase 11 must be cleared in Phase 13 before `npm run typecheck` can be added to the UI hook section. After Phase 13, add `&& npm run typecheck` to the UI section in `.githooks/pre-commit`.

### Timing

Measured 2026-02-25 on development machine:

| Sub-project | Command | Wall time |
|-------------|---------|-----------|
| TypeScript runtime | `npm run typecheck` (tsc --noEmit) | ~3.5s |
| TypeScript runtime | `npm run lint:check` (oxlint) | ~0.1s |
| TypeScript runtime | **Total** | **~3.6s** |
| Svelte UI | `npm run lint:check` (oxlint + eslint-plugin-svelte) | ~0.1s |
| Python sidecar | `uv run ruff check .` | ~0.5s |
| Python sidecar | `uv run pyright` | ~3.4s |
| Python sidecar | **Total** | **~4.0s** |

All sub-projects are well under the 10-second threshold (HOOK-03).

### Install

```bash
git config core.hooksPath .githooks
```

Run this once after cloning. See also `CONTRIBUTING.md`.
