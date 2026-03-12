# Aletheia Code Standards

> Single source of truth for all languages. `.claude/rules/*.md` files are excerpts of this document.

---

## Philosophy

Code is the documentation. Names, types, and structure carry meaning. Comments explain *why*, never *what*. If code needs a comment to explain what it does, rewrite the code.

Fail fast, fail loud. Crash on invariant violations. No defensive fallbacks for impossible states. Sentinel values and silent degradation are bugs. Surface errors at the point of origin with full context.

Anthropic-first. hermeneus models the Anthropic Messages API surface natively — not a lowest-common-denominator abstraction. If Anthropic supports a feature (caching, citations, tool_choice, token counting, batch), hermeneus exposes it. Other providers get adapter shims that map to what they support. We never discard Anthropic capabilities to be "provider-agnostic" for providers we don't use.

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
| Files | `snake_case` (Rust) / `kebab-case` (scripts) | `session_store.rs`, `deploy.sh` |
| Types / Traits / Classes | `PascalCase` | `SessionStore`, `EmbeddingProvider` |
| Functions / Methods | `snake_case` | `load_config` |
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
- File headers: one line describing purpose (`//!` module doc comment)

Doc comments (rustdoc `///`, JSDoc `/** */`) only on:
- Public API items that cross module boundaries
- `unsafe` functions (mandatory `# Safety` section)
- Functions that can panic (mandatory `# Panics` section)
- Functions returning `Result` with non-obvious error conditions

Structured inline comment categories (use the prefix exactly as shown):

| Prefix | When to use |
|--------|-------------|
| `// SAFETY:` | Precedes every `unsafe` block — explains why it is sound |
| `// INVARIANT:` | Documents a maintained invariant at a call site or type definition |
| `// NOTE:` | Non-obvious context that does not fit the surrounding code's logic |
| `// TODO(#NNN):` | Known gap, must reference a tracking issue number |
| `// FIXME(#NNN):` | Temporary workaround pending a fix, must reference an issue |

No bare `// TODO` or `// FIXME` without an issue number — they become invisible debt.

### Testing (Universal)

Behavior over implementation:
- Test what the code does, not how it does it
- One logical assertion per test
- Descriptive names: `returns_empty_when_session_has_no_turns`, not `test_add` or `it_works`
- Same-directory test files (`#[cfg(test)] mod tests`)

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

> **Decision record:** [ADR-001: snafu for Error Handling](ADR-001-errors.md) — why snafu over thiserror/anyhow, alternatives considered, crate inventory.

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

# Correctness: holding std::sync::Mutex across .await causes deadlocks
await_holding_lock = "deny"

# High-value warnings
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
# string_to_string removed — covered by implicit_clone since clippy 1.80
trait_duplication_in_bounds = "warn"
unused_self = "warn"
```

### Feature Flags

Features are additive. Never negative names. Binary crate is the feature aggregator.

```toml
# Illustrative pattern — see crates/aletheia/Cargo.toml for actual binary features
[features]
default = ["tui"]
knowledge-store = ["aletheia-nous/knowledge-store", "aletheia-mneme/mneme-engine"]
tui = ["dep:aletheia-theatron-tui"]
tls = ["aletheia-pylon/tls"]
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
[graph]
exclude = ["aletheia-mneme-engine", "aletheia-integration-tests"]

[advisories]
ignore = [
    { id = "RUSTSEC-2023-0071", reason = "rsa timing side-channel via jsonwebtoken — no safe upgrade, local-only use" },
    { id = "RUSTSEC-2025-0057", reason = "fxhash unmaintained — transitive via graph_builder, no safe upgrade" },
    # additional entries — see deny.toml for the full list
]

[licenses]
allow = [
    "MIT", "Apache-2.0", "AGPL-3.0-or-later",
    "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib",
    "Unicode-3.0", "BSL-1.0", "CC0-1.0", "0BSD",
    "MPL-2.0", "LGPL-3.0-or-later", "NCSA", "CDLA-Permissive-2.0", "OpenSSL",
]

[bans]
multiple-versions = "warn"
wildcards = "allow"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-git = ["https://github.com/romnn/reqwest-eventsource"]
```

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

### Rule: Crate Dependency Direction (layered graph)

**What:** Dependencies flow from higher layers to lower layers only. See `docs/ARCHITECTURE.md` for the full dependency table.

**Why:** Circular dependencies cause compilation errors and tight coupling between crates that should be independent. Cargo enforces this at build time, but the *intent* of the layering matters too.

**Compliant:**
```rust
// In nous (higher layer): may depend on organon, hermeneus, taxis, koina
use aletheia_organon::ToolRegistry;
use aletheia_hermeneus::LlmProvider;
```

**Non-compliant:**
```rust
// In koina (lowest layer): must not depend on any other workspace crate
use aletheia_taxis::Config; // forbidden — koina is a leaf node
```

**Enforced by:** Cargo (compile-time) + convention. See `docs/ARCHITECTURE.md#dependency-rules`.


---

### Rule: Event Name Format noun:verb

**What:** All event names use `noun:verb` format (e.g., `turn:before`, `tool:called`, `session:created`). No other formats.

**Why:** Consistent `noun:verb` naming makes event subscriptions greppable and predictable.

**Enforced by:** Convention.


---

### Rule: Visibility Discipline

**What:** `pub(crate)` by default. `pub` only for items that cross crate boundaries. Each crate's public API should be the minimal surface needed by its dependents.

**Why:** Every `pub` item is a commitment. Minimizing the public surface reduces coupling and makes refactoring tractable.

**Enforced by:** Convention + code review.


---

## Enforcement Summary Table

| Rule | Status |
|------|--------|
| Gnomon Naming Convention | Active |
| Module Import Direction | Convention |
| Event Name Format | Convention |
| Logger Creation Pattern | Convention |
| Module Boundary Discipline | Convention |

---

## Pre-commit Hooks

Gitleaks runs via `.pre-commit-config.yaml` to catch credential leaks. Install with `pre-commit install`.

The instance guard (`scripts/pre-commit-instance-guard`) prevents accidental commits of `instance/` files. Install manually if desired:

```bash
cp scripts/pre-commit-instance-guard .git/hooks/pre-commit
```

Lint and type checks run in CI. Run them locally before pushing:

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p <affected-crate>
```
