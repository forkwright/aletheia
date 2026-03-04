# Aletheia Code Standards

> Single source of truth for all languages. `.claude/rules/*.md` files are excerpts of this document.

---

## Philosophy

Code is the documentation. Names, types, and structure carry meaning. Comments explain *why*, never *what*. If code needs a comment to explain what it does, rewrite the code.

Fail fast, fail loud. Crash on invariant violations. No defensive fallbacks for impossible states. Sentinel values and silent degradation are bugs. Surface errors at the point of origin with full context.

Parse, don't validate. Invalid data cannot exist. Newtypes with validation constructors enforce invariants at construction time. Once a value is constructed, its validity is a type-level guarantee.

Minimize surface area. `pub(crate)` by default (Rust), unexported by default (TS). Every public item is a commitment. Expose the smallest API that serves the need.

---

## Universal Rules

These apply regardless of language.

### Naming

#### Gnomon System (Persistent Names)

Module directories, agent identities, subsystems, and major features follow the gnomon naming convention. Names identify essential natures, not implementations. Pass the layer test (L1-L4). If no Greek word fits naturally, the essential nature isn't clear yet - wait.

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
1. Carry context - what operation failed, with what inputs
2. Be typed - callers can match on error kind
3. Propagate - chain errors with `.context()` or equivalent, never swallow
4. Surface - log at the point of handling, not the point of throwing

Fail fast:
- Panic on programmer bugs (violated invariants, impossible states)
- `Result` / `throw` for anything the caller could handle or report
- `expect("invariant description")` (Rust) over bare `unwrap()` - the message documents the invariant
- Never panic in library code for recoverable errors

No silent catch:
- Every catch/match block must log, propagate, return a meaningful value, or explain why it's discarded
- `/* intentional: reason */` for deliberate discard - never empty catch

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

### Test Data Policy

All test data MUST be synthetic. No real personal information in test fixtures, assertions, or example data.

**Standard test identities:**
- Users: `alice`, `bob`, `charlie`
- Emails: `alice@example.com`, `bob@acme.corp`
- IPs: `192.168.1.100`, `10.0.0.1`
- Domains: `example.com`, `acme.corp`
- Facts: Use invented specs ("Widget torque 42 Nm"), generic projects ("Project Alpha"), fictional deadlines

**Never use in test code:**
- Real names, emails, or usernames
- Real internal IPs or hostnames
- Real personal facts (health, family, education, employment)
- Real credentials or API keys

The pre-commit hook and CI pipeline reject PRs containing known PII patterns.

### Module Boundaries

Imports flow from higher layers to lower layers only. Never create cycles. Verify the dependency graph before adding any cross-module import.

Each module declares its public surface explicitly. Consumers import from the public API, not internal files.

### Events

All event names: `noun:verb` format. No exceptions.
- `turn:before`, `tool:called`, `session:created`, `distillation:complete`
- Use module name as noun for lifecycle events: `plugin:loaded`, `daemon:started`

### Git & Workflow

#### Commits

Conventional commits with module scope:
```text
feat(mneme): add WAL checkpoint scheduling
fix(hermeneus): correct token counting for tool results
refactor(organon): extract tool validation into separate module
test(nous): add property tests for message distillation
perf(mneme): use cached prepared statements
```

Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `ci`, `perf`

Rules:
- Present tense, imperative mood: "add X" not "added X"
- First line <=72 characters
- Body wraps at 80 characters
- One logical change per commit
- Squash merge on PR - branch preserves detailed history

#### Authorship

All commits use the operator's identity. Agents are tooling, not contributors.

#### Branches

| Type | Pattern | Example |
|------|---------|---------|
| Feature | `feat/<description>` | `feat/embedding-provider` |
| Bug fix | `fix/<description>` | `fix/distillation-overflow` |
| Spec work | `spec<NN>/<description>` | `spec43/mneme-crate` |
| Chore/docs | `chore/<description>` | `chore/update-deps` |

Branch from `main`. Rebase before pushing. Never commit directly to `main`.

#### Decision Documentation

Significant design decisions get a document in `docs/`. Include: context, options considered, decision, consequences.

---

## Rust

### Edition & Toolchain

- Edition: **2024** (latest stable)
- Resolver: **2** (mandatory in workspace)
- MSRV: latest stable (internal project - no MSRV ceremony)
- Async: **Tokio** runtime, native `async fn` in traits (no `async-trait` crate)

### Error Handling (Rust)

Per-crate error types via `snafu` (GreptimeDB pattern - context wrapping, Location-based virtual stack traces):

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
        source: serde_yaml::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

fn load_config(path: &Path) -> Result<Config, ConfigError> {
    let contents = std::fs::read_to_string(path)
        .context(ReadConfigSnafu { path: path.display().to_string() })?;
    let config: Config = serde_yaml::from_str(&contents)
        .context(ParseConfigSnafu)?;
    Ok(config)
}
```

Layering:
- `snafu` in library crates - typed, matchable, with Location tracking
- `anyhow` only in `main()`, CLI entry points, and tests - never in library crates
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
- Known finite variants -> enum with exhaustive match
- Runtime-extensible behavior -> trait

**`#[non_exhaustive]` on public enums** that may grow.

**Visibility:** `pub(crate)` by default. `pub` only for public API items. Private by default.

### Allocation Awareness

| Situation | Use | Avoid |
|-----------|-----|-------|
| Read-only string input | `&str` | `String` |
| Mostly borrowed, sometimes owned | `Cow<'_, str>` | `String` |
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

- `#[expect(lint)]` over `#[allow(lint)]` - warns when suppression becomes unnecessary
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
- Imports grouped: std -> external -> workspace -> local, blank line between groups

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

**What:** All thrown errors extend `AletheiaError`. Never `throw new Error(...)` or throw strings. Non-critical operations use `trySafe`/`trySafeAsync` from `koina/safe.ts`.

**Why:** Bare errors are uncatchable by type - callers cannot distinguish error categories. The typed hierarchy enables targeted handling, structured logging, and clean propagation across the call stack.

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

**Enforced by:** Convention + agent context. See `koina/errors.ts` for the full hierarchy and `koina/error-codes.ts` for codes.


---

### Rule: No Silent Catch

**What:** Every catch block must log the error, rethrow it, return a meaningful value, or include an explicit `/* reason */` comment explaining the intentional discard.

**Why:** Silent catch blocks bury failures. In a daemon process there is no interactive feedback - a swallowed error is an invisible bug.

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

**Enforced by:** `no-empty` (oxlint, currently `error`) catches empty catch blocks. Silent catch with a body (e.g., comment-only block) is convention + agent context.


---

### Rule: No Explicit Any

**What:** Never use `any` as a type annotation. Use `unknown` with type narrowing, or define a proper interface/type.

**Why:** `any` disables the type checker for the annotated value and everything downstream. In a typed codebase it creates invisible holes in the type system.

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

**Enforced by:** `typescript/no-explicit-any` (oxlint).


---

### Rule: Logger Not Console

**What:** Use `createLogger("module-name")` for all daemon logging. `console.*` is acceptable only in CLI-mode functions that produce human-readable stdout (e.g., `nous/audit.ts`).

**Why:** `createLogger` includes structured context (session ID, turn ID, agent ID) via AsyncLocalStorage. `console.*` in daemon code loses all context correlation and produces unstructured output that cannot be filtered or aggregated.

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

**Enforced by:** `no-console` (oxlint). Exception: `nous/audit.ts` and any function explicitly documented as producing CLI stdout output.


---

### Rule: Typed Promise Returns on Sync ToolHandler Branches

**What:** `ToolHandler.execute()` implementations that are synchronous in some branches must use `return Promise.resolve(result)` rather than the `async` keyword without any `await`.

**Why:** `async` on a function with no `await` triggers `eslint(require-await)`. The fix: keep the `Promise<string>` return type, drop the `async` keyword. Do not remove `async` and change the return type - that breaks the `ToolHandler` interface contract.

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

**Enforced by:** `require-await` (oxlint). See CONTRIBUTING.md Gotcha 3.


---

### Rule: Sort Named Imports Within Statement

**What:** Named imports within a single `import { }` statement must be sorted alphabetically (case-insensitive).

**Why:** Consistent ordering reduces diff noise and makes scanning for specific names faster.

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

**Enforced by:** `sort-imports` (oxlint). Note: `ignoreDeclarationSort: true` means statement-level ordering is not enforced - only member-level sorting within a single import statement.


---

### Rule: .js Import Extensions

**What:** All relative imports must include the `.js` extension, even for `.ts` source files.

**Why:** TypeScript with `"moduleResolution": "bundler"` and ESM output requires `.js` extensions for Node.js compatibility. Omitting them causes runtime module resolution failures in built output.

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

**Enforced by:** tsconfig `moduleResolution` (fails to resolve extensionless imports at build time).


---

### Rule: Type-Only Imports with `import type`

**What:** Import type-only symbols using `import type { }` syntax, not `import { }`. When mixing value and type imports from the same module, use inline `type` modifier: `import { value, type MyType }`.

**Why:** `import type` is erased at compile time - no runtime module load. Mixing runtime and type imports creates unnecessary module dependencies and increases bundle size.

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


---

### Rule: No Floating Promises

**What:** Every `Promise` returned by an `async` function must be `await`ed, returned, or explicitly handled. No fire-and-forget unless intent is documented.

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


---

## Svelte

### Rule: No XSS via @html

**What:** Never use `{@html ...}` with user-supplied or externally-sourced content unless sanitized through a verified library.

**Why:** `{@html}` bypasses Svelte's automatic HTML escaping and renders raw HTML directly into the DOM. Unsanitized user content creates XSS vulnerabilities.

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

**Enforced by:** `svelte/no-at-html-tags` (eslint-plugin-svelte). Convention + agent context.


---

### Rule: Svelte 5 Runes Only (no legacy reactive syntax)

**What:** Use Svelte 5 rune syntax (`$state`, `$derived`, `$effect`, `$props`) exclusively. No legacy reactive declarations (`$:`, `export let`, reactive stores with `$storeName` auto-subscription in script blocks).

**Why:** Aletheia targets Svelte 5. Legacy reactive syntax is deprecated and will be removed. Mixing syntaxes creates ambiguous component behavior and blocks future upgrades.

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


---

### Rule: svelte-check Warnings Are Errors

**What:** `svelte-check` must pass with zero warnings in CI. Warnings are errors for gating purposes.

**Why:** `svelte-check` catches type errors, missing props, and deprecated API usage. Letting warnings accumulate creates silent debt that compounds over time.

**Compliant:**
All props typed, no deprecated APIs, `svelte-check` exits 0.

**Non-compliant:**
Any `svelte-check` warning left unaddressed; `svelte-check` run without `--fail-on-warnings`.

**Enforced by:** CI step `cd ui && npx svelte-check --fail-on-warnings`.


---

### Rule: Typed Component Props

**What:** Type all Svelte component props using `$props<{ ... }>()` with a TypeScript interface or inline type. No untyped or `any`-typed props.

**Why:** Untyped props break type-checking at component boundaries. TypeScript cannot verify that parent components pass correct prop types.

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


---

## Python (Memory Sidecar)

### Rule: FastAPI Depends() Pattern (not B008)

**What:** Use `fastapi.Depends()` for dependency injection in FastAPI route signatures. No mutable default arguments or direct function calls in parameter defaults.

**Why:** FastAPI's `Depends()` handles lifecycle, caching, and async context correctly. Calling functions directly in default arguments (ruff rule B008) executes them at module import time, not per-request.

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

**Enforced by:** ruff rule `B008`. FastAPI pattern is also validated by `pyright` strict mode.


---

### Rule: Ruff-Selected Rule Set

**What:** The sidecar must pass ruff lint with rule sets: `E`, `W`, `F`, `B`, `I`, `UP`. No `# noqa` suppression without an inline explanation.

**Why:** These rule sets cover pyflakes errors (`F`), pycodestyle (`E`/`W`), bugbear patterns (`B`), import ordering (`I`), and pyupgrade modernization (`UP`).

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

**Enforced by:** ruff (`pyproject.toml`).


---

### Rule: Pyright Strict Mode

**What:** The sidecar must pass `pyright --strict` with zero errors. All functions need explicit return type annotations and typed parameters.

**Why:** Untyped FastAPI route parameters produce incorrect OpenAPI schemas and miss runtime validation errors. Pyright strict catches these at development time.

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

**Enforced by:** pyright strict mode.


---

### Rule: No Bare Exception Catch

**What:** No bare `except:` or `except Exception:` without re-raising or logging with context. Catch the most specific exception type available.

**Why:** Bare `except:` catches `SystemExit`, `KeyboardInterrupt`, and other non-error signals. Even `except Exception:` swallows errors silently without proper handling. FastAPI routes with silent exception swallowing return 500s with no diagnostic information.

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

**Enforced by:** ruff rule `BLE001` (blind exception catch) and `E722` (bare except).


---

## Naming

### Rule: Gnomon Naming Convention

**What:** Persistent names for modules, subsystems, agents, and major components follow the gnomon naming convention. Names unconceal essential natures, pass the layer test (L1-L4), and compose with the existing name topology.

**Why:** The naming system is not decoration. Names that identify the right essential nature survive refactors, communicate architectural intent, and resist drift toward generic labels. A well-chosen name teaches you something about what it names.

**Applies to:** Module directories, agent identities, subsystem names, major persistent features. Does *not* apply to: utility functions, variable names, temporary branches, test fixtures.

**Process:**
1. Identify the mode of attention (not the implementation)
2. Construct from Greek roots using prefix-root-suffix system
3. Run the layer test (L1 practical through L4 reflexive)
4. Check topology against existing names
5. If no Greek word fits naturally, the mode of attention isn't clear yet - wait

**Enforced by:** Convention + agent context.

---

## Architecture

### Rule: Module Import Direction (layered graph)

**What:** Imports flow from higher layers to lower layers only. See `docs/ARCHITECTURE.md` for the full dependency table.

**Why:** Circular imports cause initialization order bugs and tight coupling between modules that should be independent. Node.js ESM does not handle circular dependencies cleanly - they produce `undefined` values at import time.

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

**Enforced by:** `import/no-cycle` (oxlint). Convention + agent context for directional enforcement. See `docs/ARCHITECTURE.md#dependency-rules`.


---

### Rule: Event Name Format noun:verb

**What:** All event bus event names use `noun:verb` format (e.g., `turn:before`, `tool:called`, `session:created`). No other formats.

**Why:** Consistent `noun:verb` naming makes event subscriptions greppable and predictable. Mixed formats (camelCase, hyphenated, colon-less) make it impossible to find all events for a subsystem.

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

**Enforced by:** Convention + agent context.


---

### Rule: Logger Creation Pattern createLogger("module-name")

**What:** Create loggers at module scope using `createLogger("module-name")`. The module name must match the module's directory name or use `"module:subcomponent"` for sub-components.

**Why:** The module name appears in every log line and drives log filtering. Inconsistent naming makes cross-module correlation impossible.

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

**Enforced by:** Convention + agent context. `createLogger` is the only way to get a structured logger - no alternative exists in the codebase.


---

### Rule: Module Boundary Discipline

**What:** Import directly from the file that owns the symbol. Do not create `index.ts` barrel files that re-export a module's internals just to flatten import paths. Modules with a legitimate public API surface (e.g., `dianoia/index.ts` that defines the module's external interface) may use an `index.ts` to curate their exports.

**Why:** Gratuitous barrel files load entire modules when only one export is needed, hide the true dependency graph, and make tree-shaking harder. Direct imports keep dependencies explicit.

**Compliant:**
```typescript
// Direct import from the file that owns the symbol
import { SessionStore } from "../mneme/store.js";
import { createLogger } from "../koina/logger.js";
import { AletheiaError } from "../koina/errors.js";

// Module with a curated public API surface — acceptable
import { orchestrate } from "../dianoia/index.js";
```

**Non-compliant:**
```typescript
// Gratuitous barrel that re-exports everything:
// koina/index.ts re-exporting createLogger, AletheiaError, trySafe, etc.
import { createLogger } from "../koina/index.js";

// Barrel created just to flatten paths:
// mneme/index.ts re-exporting SessionStore, makeDb, etc.
import { SessionStore } from "../mneme/index.js";
```

**Enforced by:** Convention + agent context.


---

## Enforcement Summary Table

| Rule | Status |
|------|--------|
| Typed Errors Only | Convention |
| No Silent Catch | Active |
| No Explicit Any | Active |
| Logger Not Console | Convention |
| Typed Promise Returns | Active |
| Sort Named Imports | Active |
| Prefer await over .then() | Active |
| Catch param naming | Active |
| .js Import Extensions | Active |
| Type-Only Imports | Active |
| No Floating Promises | Active |
| No XSS via @html | Convention |
| Svelte 5 Runes Only | Convention |
| svelte-check Warnings | Convention |
| Typed Component Props | Convention |
| FastAPI Depends() | Convention |
| Ruff Rule Set | Convention |
| Pyright Strict Mode | Convention |
| No Bare Exception Catch | Convention |
| Gnomon Naming Convention | Active |
| Module Import Direction | Convention |
| Event Name Format | Convention |
| Logger Creation Pattern | Convention |
| Module Boundary Discipline | Convention |

---

## Pre-commit Hook

`.githooks/pre-commit` runs lint and type checks for each sub-project when relevant files are staged. The full test suite runs in CI only.

### What runs

| Sub-project | Trigger | Commands |
|-------------|---------|----------|
| TypeScript runtime | Any `infrastructure/runtime/` file staged | `npm run typecheck && npm run lint:check` |
| Svelte UI | Any `ui/` file staged | `npm run lint:check` (oxlint + eslint-plugin-svelte) |
| Python sidecar | Any `infrastructure/memory/sidecar/` file staged | `uv run ruff check . && uv run pyright` |

### Timing

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
