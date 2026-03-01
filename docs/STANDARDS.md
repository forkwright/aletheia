# Aletheia Code Standards

> Single source of truth for all languages. `.claude/rules/*.md` files are excerpts of this document.
> Each rule: what / why / compliant / non-compliant / enforced-by / scan count (where applicable).
> Last updated: 2026-02-28

---

## Philosophy

Code is the documentation. Names, types, and structure carry meaning. Comments explain *why*, never *what*. If code needs a comment to explain what it does, the code needs refactoring.

Fail fast, fail loud. Crash on invariant violations. No defensive fallbacks for impossible states. Sentinel values and silent degradation are bugs. Surface errors at the point of origin with full context.

Parse, don't validate. Invalid data cannot exist. Newtypes with validation constructors enforce invariants at construction time. Once a value is constructed, its validity is a type-level guarantee.

Minimize surface area. `pub(crate)` by default (Rust), unexported by default (TS). Every public item is a commitment. Expose the smallest API that serves the need.

---

## Universal Rules

These apply regardless of language.

### Naming

#### Gnomon System (Persistent Names)

Module directories, agent identities, subsystems, and major features follow the [gnomon naming system](gnomon.md). Names identify modes of attention, not implementations. Pass the layer test (L1-L4). If no Greek word fits naturally, the mode of attention isn't clear yet — wait.

Applies to: modules, agents, subsystems, features that persist.
Does not apply to: variables, functions, test fixtures, temporary branches.

#### Code Names

| Context | Convention | Example |
|---------|-----------|---------|
| Files | `kebab-case` | `session-store.rs`, `event-bus.ts` |
| Types / Traits / Classes | `PascalCase` | `SessionStore`, `EmbeddingProvider` |
| Functions / Methods | `camelCase` (TS) / `snake_case` (Rust/Python) | `loadConfig` / `load_config` |
| Constants | `UPPER_SNAKE` | `MAX_TURNS`, `DEFAULT_PORT` |
| Events | `noun:verb` | `turn:before`, `tool:called` |

Verb-first for functions: `load_config`, `create_session`, `parse_input`. Drop `get_` prefix on getters.

### Error Handling (Universal)

Every error must:
1. Carry context — what operation failed, with what inputs
2. Be typed — callers can match on error kind
3. Propagate — chain errors with `.context()` or equivalent, never swallow
4. Surface — log at the point of handling, not the point of throwing

Fail fast:
- Panic on programmer bugs (violated invariants, impossible states)
- `Result` / `throw` for anything the caller could handle or report
- `expect("invariant description")` (Rust) over bare `unwrap()` — the message documents the invariant
- Never panic in library code for recoverable errors

No silent catch:
- Every catch/match block must log, propagate, return a meaningful value, or explain why it's discarded
- `/* intentional: reason */` for deliberate discard — never empty catch

### Documentation

Zero-comment default:
- No inline comments except genuinely non-obvious *why* explanations
- No creation dates, author info, "upgraded stack" explanations
- No AI generation indicators
- File headers: one line describing purpose (Rust: `//!` module doc, TS/Python: single-line comment)

Doc comments (rustdoc `///`, JSDoc `/** */`) only on:
- Public API items that cross module boundaries
- `unsafe` functions (mandatory `# Safety` section)
- Functions that can panic (mandatory `# Panics` section)
- Functions returning `Result` with non-obvious error conditions

### Testing (Universal)

Behavior over implementation:
- Test what the code does, not how it does it
- One logical assertion per test
- Descriptive names: `returns_empty_when_session_has_no_turns`, not `test_add` or `it_works`
- Same-directory test files (`foo.test.ts`, `#[cfg(test)] mod tests`)

Property-based testing:
- Serialization round-trips, algebraic properties, state machine invariants
- proptest / bolero (Rust) / fast-check (TS) for edge case discovery
- Example tests document expected behavior; property tests catch the unexpected

No coverage targets. Coverage is a vanity metric. Test the behavior that matters.

### Module Boundaries

Imports flow from higher layers to lower layers only. Never create cycles. Adding a cross-module import requires verifying the dependency graph.

Each module declares its public surface explicitly. Consumers import from the public API, not internal files.

### Events

All event names: `noun:verb` format. No exceptions.
- `turn:before`, `tool:called`, `session:created`, `distillation:complete`
- Use module name as noun for lifecycle events: `plugin:loaded`, `daemon:started`

### Git & Workflow

#### Commits

Conventional commits with module scope:
```
feat(mneme): add WAL checkpoint scheduling
fix(hermeneus): correct token counting for tool results
refactor(organon): extract tool validation into separate module
test(nous): add property tests for message distillation
perf(mneme): use cached prepared statements
```

Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `ci`, `perf`

Rules:
- Present tense, imperative mood: "add X" not "added X"
- First line ≤72 characters
- Body wraps at 80 characters
- One logical change per commit
- Squash merge on PR — branch preserves detailed history

#### Authorship

All commits use Alice's identity. Agents are tooling, not contributors.

#### Branches

| Type | Pattern | Example |
|------|---------|---------|
| Feature | `feat/<description>` | `feat/embedding-provider` |
| Bug fix | `fix/<description>` | `fix/distillation-overflow` |
| Spec work | `spec<NN>/<description>` | `spec43/mneme-crate` |
| Chore/docs | `chore/<description>` | `chore/update-deps` |

Always branch from `main`. Always rebase before pushing. Never commit directly to `main`.

#### Decision Documentation

Significant design decisions get a document in `docs/specs/` or `docs/decisions/`. Include: context, options considered, decision, consequences.

---

## Rust

### Edition & Toolchain

- Edition: **2024** (latest stable)
- Resolver: **2** (mandatory in workspace)
- MSRV: latest stable (internal project — no MSRV ceremony)
- Async: **Tokio** runtime, native `async fn` in traits (no `async-trait` crate)

### Error Handling (Rust)

Per-crate error types via `snafu` (GreptimeDB pattern — context wrapping, Location-based virtual stack traces):

```rust
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub enum ConfigError {
    #[snafu(display("failed to read config from {path}"))]
    ReadConfig {
        path: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("failed to parse config"))]
    ParseConfig {
        source: serde_yml::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

fn load_config(path: &Path) -> Result<Config, ConfigError> {
    let contents = std::fs::read_to_string(path)
        .context(ReadConfigSnafu { path: path.display().to_string() })?;
    let config: Config = serde_yml::from_str(&contents)
        .context(ParseConfigSnafu)?;
    Ok(config)
}
```

Layering:
- `snafu` in library crates — typed, matchable, with Location tracking
- `anyhow` only in `main()`, CLI entry points, and tests — never in library crates
- `miette` for user-facing diagnostics
- Convention: `source` field = internal error (walk chain), `error` field = external (stop walking)
- Log errors where HANDLED, not where they occur

### Types & Patterns

**Newtypes for domain identifiers:**

```rust
pub struct AgentId(compact_str::CompactString);
pub struct SessionId(ulid::Ulid);
pub struct TurnId(u64);
```

No raw `String` or `u64` for identifiers. Construction validates. The type *is* the documentation.

**Typestate for state machines:**

```rust
pub struct Session<S: SessionState> {
    id: SessionId,
    agent: AgentId,
    _state: PhantomData<S>,
}
pub struct Idle;
pub struct Active;

impl Session<Idle> {
    pub fn begin_turn(self) -> Session<Active> { /* ... */ }
}
impl Session<Active> {
    pub fn complete(self) -> Session<Idle> { /* ... */ }
    // Session<Idle> has no send_message() — compile error
}
```

**Enums for closed sets, traits for open sets:**
- Known finite variants → enum with exhaustive match
- Runtime-extensible behavior → trait

**`#[non_exhaustive]` on public enums** that may grow.

**Visibility:** `pub(crate)` by default. `pub` only for public API items. Private by default.

### Allocation Awareness

| Situation | Use | Avoid |
|-----------|-----|-------|
| Read-only string input | `&str` | `String` |
| Usually borrowed, sometimes owned | `Cow<'_, str>` | `String` |
| Must own | `String` | — |
| Compile-time known | `&'static str` | `String` |
| Return from function, caller decides | `impl Into<String>` | Force `String` |

Prefer iterator pipelines over collecting into intermediate `Vec`s. No `.clone()` in hot paths without justification.

### Unsafe Policy

- `#[deny(unsafe_code)]` at workspace level
- Crates needing `unsafe`: explicit `#![allow(unsafe_code)]` at crate root with justification
- Every `unsafe` block requires `// SAFETY:` comment
- Expected: `mneme` (FFI/perf), `semeion` (signal IPC), `prostheke` (WASM host)
- All others: `#[forbid(unsafe_code)]`

### Lint Suppression

- `#[expect(lint)]` over `#[allow(lint)]` — warns when suppression becomes unnecessary
- Every suppression requires inline reason: `#[expect(clippy::too_many_arguments, reason = "builder pattern WIP")]`

### Credentials

```rust
use secrecy::{ExposeSecret, SecretString};

struct ProviderConfig {
    api_key: SecretString,  // zeroized on drop, redacted in Debug
    endpoint: String,
}
```

`expose_secret()` is the only way to access the value.

### Workspace Lints

Single source of truth in workspace `Cargo.toml`. All crates inherit with `[lints] workspace = true`.

```toml
[workspace.lints.rust]
future_incompatible = "warn"
nonstandard_style = "warn"
rust_2018_idioms = "warn"
unsafe_code = "deny"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
missing_errors_doc = "allow"
missing_panics_doc = "allow"
module_name_repetitions = "allow"
must_use_candidate = "allow"

# Deny-level
dbg_macro = "deny"
todo = "deny"
unimplemented = "deny"
exit = "deny"

# High-value warnings
await_holding_lock = "warn"
explicit_into_iter_loop = "warn"
fallible_impl_from = "warn"
fn_params_excessive_bools = "warn"
implicit_clone = "warn"
large_types_passed_by_value = "warn"
map_err_ignore = "warn"
match_wildcard_for_single_variants = "warn"
needless_for_each = "warn"
rc_mutex = "warn"
string_add = "warn"
string_to_string = "warn"
trait_duplication_in_bounds = "warn"
unused_self = "warn"
```

### Feature Flags

Features are additive. Never negative names. Binary crate is the feature aggregator.

```toml
[features]
default = ["signal", "fastembed", "graph"]
signal = ["dep:signal-ipc"]
browser = ["dep:chromiumoxide"]
graph = ["dep:cozo"]
fastembed = ["dep:fastembed"]
voyage = ["dep:reqwest"]
wasm-plugins = ["dep:wasmtime"]
```

### Conversion Methods

| Prefix | Cost | Ownership | Example |
|--------|------|-----------|---------|
| `as_` | Free | `&self → &T` | `as_bytes()` |
| `to_` | Expensive | `&self → T` | `to_lowercase()` |
| `into_` | Variable | `self → T` | `into_bytes()` |

### File Organization

- One primary type per file when substantial (>100 lines)
- Named files (`session.rs`) over `session/mod.rs` unless module has submodules
- Tests in `#[cfg(test)] mod tests` at bottom of source file
- Integration tests in `tests/` directory per crate
- Imports grouped: std → external → workspace → local, blank line between groups

### Rust Imports

```rust
use std::collections::HashMap;
use std::sync::Arc;

use snafu::ResultExt;
use tokio::sync::mpsc;

use koina::error::AletheiaError;
use taxis::config::Config;

use crate::session::SessionStore;
use super::handler::MessageHandler;
```

---

## CI Pipeline

Four stages, fail-fast ordering:

### Stage 1: Fast Checks (seconds)
```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
```

### Stage 2: Build & Test (minutes)
```bash
cargo nextest run --workspace
cargo test --doc
cargo hack check --each-feature --no-dev-deps
```

### Stage 3: Deep Checks (PR merge gate)
```bash
cargo audit
cargo semver-checks check-release  # if publishing crates
```

### Stage 4: Nightly/Weekly
```bash
cargo +nightly miri test -p aletheia-koina -p aletheia-taxis
cargo fuzz run <target> -- -max_total_time=300
cargo bench  # regression detection via divan
```

### Supply Chain (`deny.toml`)
```toml
[advisories]
vulnerability = "deny"
unmaintained = "warn"
yanked = "warn"

[licenses]
unlicensed = "deny"
allow = ["MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "Unicode-3.0"]
copyleft = "deny"

[bans]
multiple-versions = "warn"
wildcards = "deny"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

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

## Project Governance

Rules specific to the Rust rewrite project lifecycle.

### Implementation Philosophy: Docs Are the Spec

When implementing each crate:

1. Read the relevant section of `docs/PROJECT.md`
2. Read `docs/ARCHITECTURE.md` for boundary rules
3. Read this document for invariants
4. Read `docs/gnomon.md` for naming
5. Implement from those documents
6. Consult TS/Python code only to understand *intent*, not to copy implementation

Known-wrong patterns do not carry forward: per-request DB connections, `execSync`, `appendFileSync`, mem0 monkey-patching, silent catches, bare `throw new Error`.

### Commit Standards

```
<type>(<scope>): <imperative description, ≤72 chars>

<what and why, wrapped at 72 chars>
```

Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `security`. Scopes: crate names + `ui`, `tui`, `cli`, `specs`, `ci`. Single author: `forkwright <alice@example.com>`. No `Co-authored-by` lines. No agent attribution.

### Deviation Rules

When implementing, deviations from the plan or existing specs follow escalation levels:

| Level | Scope | Action |
|-------|-------|--------|
| L1 | Bug fix — broken behavior, no design change | Auto-fix, document in commit |
| L2 | Critical addition — missing piece that blocks progress | Auto-add, document rationale in commit body |
| L3 | Blocker resolution — spec conflict or impossible requirement | Auto-fix, flag in next status update |
| L4 | Design change — different approach than what's specified | **STOP AND ASK.** No autonomous design changes. |

### Research Protocol

Claims about external systems, libraries, or protocols require evidence:

| Tier | Source | Example |
|------|--------|---------|
| S1 | Peer-reviewed / official docs | Tokio docs, Rust reference, RFC |
| S2 | Authoritative secondary | crates.io README, well-maintained blog |
| S3 | Community knowledge | GitHub issues, Stack Overflow with verification |
| S4 | Direct testing | "I ran this and observed..." |
| S5 | Our synthesis | Combining sources into a conclusion |

**Rules:** Inline-cite sources. Include counter-evidence when it exists. Never cite what you haven't read. "I don't know" is always acceptable; wrong is not.

### What Carries Forward Unchanged

- **Svelte 5 UI** — no reason to rewrite
- **CozoDB** — absorbed as mneme-engine (replaces Qdrant + Neo4j)
- **signal-cli** — JVM process unchanged, Rust rewrites the glue
- **Agent workspace files** — SOUL.md, TELOS.md, MNEME.md, etc. Same files, oikos paths
- **HTTP/SSE API surface** — same endpoints, same events. UI works without modification
- **6-cycle self-improvement loop** — evolution, competence, skills, feedback, distillation, consolidation
- **Gnomon naming** — the naming system is the architecture

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
