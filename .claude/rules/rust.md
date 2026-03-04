# Rust Rules

Agent-action rules for the Aletheia Rust rewrite. All crates under `crates/`.
Integrated from QA docs 03 (reference guide) and 04 (pitfalls).

---

## Error Handling

We use **snafu** (not thiserror) for library crate error enums. GreptimeDB pattern.

- `snafu` enums per crate with `.context()` propagation and `Location` tracking
- No `unwrap()` in library code. `anyhow` only in CLI entry points.
- Convention: `source` field = internal error (walk chain), `error` field = external (stop walking)
- Log errors where HANDLED, not where they occur

Compliant:
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

Non-compliant:
```rust
// unwrap in library code
pub fn parse_config(input: &str) -> Config {
    serde_json::from_str(input).unwrap()
}

// anyhow in library - callers can't match error variants
pub fn connect(url: &str) -> anyhow::Result<Connection> { ... }

// bare ? without context - loses information
let contents = std::fs::read_to_string(path)?;

// Box<dyn Error> - erases type info
fn process() -> Result<(), Box<dyn std::error::Error>> { ... }
```

---

## Async

All I/O is async (Tokio). No `block_on` inside async context.

### Cancellation Safety

Document cancellation safety for every public async method. In `select!`:
- Cancel-SAFE: `sleep()`, `Receiver::recv()`, `Sender::reserve()`, reads into owned buffers
- Cancel-UNSAFE: `Sender::send(msg)` (message lost), `write_all()` (partial write), mutex guard across `.await`
- All `select!` branches must be cancel-safe or use reserve-then-send pattern

Compliant:
```rust
// Reserve permit first (cancel-safe), then send
let permit = tx.reserve().await?;
permit.send(message);

// Process outside select so cancellation doesn't affect it
let job = select! {
    Some(job) = rx.recv() => job,
    _ = cancel.cancelled() => break,
};
process(job).await; // runs outside select
```

Non-compliant:
```rust
// Message lost if cancelled
select! {
    _ = tx.send(msg) => {},  // cancel-UNSAFE
    _ = cancel.cancelled() => break,
}
```

### Locks Across Await

Never hold `std::sync::Mutex` guards across `.await` points. Either drop before await or use `tokio::sync::Mutex`.

Compliant:
```rust
// Scope the lock, drop before await
let data = {
    let guard = state.lock().unwrap();
    guard.clone()
};
let result = process(data).await;
```

Non-compliant:
```rust
let mut guard = state.lock().unwrap();
let data = fetch(url).await;  // deadlock: guard held across await
guard.push(data);
```

### Spawned Tasks

Spawned tasks are `'static` - they outlive any reference. Move owned data in. Clone `Arc`s before spawn. Propagate tracing spans.

Compliant:
```rust
let this = Arc::clone(&self);
let span = tracing::Span::current();
tokio::spawn(async move {
    this.handle_request().await
}.instrument(span));
```

Non-compliant:
```rust
// Won't compile - &self is not 'static
tokio::spawn(async { self.handle().await });

// Missing span propagation - loses trace context
tokio::spawn(async move { handle().await });
```

### No Nested Runtimes

Never call `Runtime::block_on()` from within async context. Use `spawn_blocking` for sync-in-async, `Handle::block_on` + `block_in_place` for async-in-sync.

---

## Lifetime & Borrowing

### No Clone Spam

The borrow checker is telling you the data flow is wrong. `.clone()` silences it without fixing the architecture.

Compliant:
```rust
// Restructure to avoid overlapping borrows
fn process(data: &mut Vec<String>) {
    let uppercased: Vec<String> = data.iter().map(|s| s.to_uppercase()).collect();
    data.extend(uppercased);
}
```

Non-compliant:
```rust
fn process(data: &mut Vec<String>) {
    let snapshot = data.clone(); // unnecessary full copy
    for item in &snapshot {
        data.push(item.to_uppercase());
    }
}
```

### Own by Default

Start with owned types. Only add lifetimes when profiling shows the allocation matters. Config structs own their strings.

```rust
// GOOD: own the data - config is long-lived
struct Config {
    name: String,
    host: String,
    port: u16,
}

// GOOD: borrow in short-lived views - justified
struct RequestView<'a> {
    path: &'a str,  // borrowing from HTTP request buffer
    method: Method,
}
```

### Use Cow for Mixed Owned/Borrowed

```rust
use std::borrow::Cow;

fn normalize_path(path: &str) -> Cow<'_, str> {
    if path.starts_with('/') {
        Cow::Borrowed(path) // no allocation
    } else {
        Cow::Owned(format!("/{path}"))
    }
}
```

### Arena Over Self-Referential Structs

Never fight the borrow checker with `RefCell` or `unsafe` for graph structures. Use arena allocation with index-based references.

```rust
struct Arena {
    nodes: Vec<Node>,
}
struct Node {
    children: Vec<usize>,  // indices into Arena.nodes
    parent: Option<usize>,
}
```

---

## Type System

### Newtypes for Domain Concepts

Domain IDs are newtype wrappers, not bare `String`/`u64`. Zero-cost, compile-time parameter swap safety.

Compliant:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentId(compact_str::CompactString);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(ulid::Ulid);

fn route_message(agent: &AgentId, session: &SessionId, msg: &str) { ... }
```

Non-compliant:
```rust
fn route_message(agent_id: &str, session_id: &str, msg: &str) { ... }
// easy to swap arguments - compiles fine, breaks at runtime
```

### #[non_exhaustive] on Public Enums

All public enums that may grow must use `#[non_exhaustive]`. Adding a variant is otherwise a breaking change.

```rust
#[non_exhaustive]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    // can add variants later without breaking downstream
}
```

### Typestate Pattern

Use typestate for multi-step builders and connection lifecycle. Compile-time state validation over runtime checks.

```rust
struct Disconnected;
struct Connected;

struct Connection<State> {
    config: Config,
    _state: std::marker::PhantomData<State>,
}

impl Connection<Disconnected> {
    fn connect(self) -> Result<Connection<Connected>> { ... }
}

impl Connection<Connected> {
    fn query(&self, q: &str) -> Result<Row> { ... }
    // query() is impossible to call on Disconnected - compile error
}
```

### Exhaustive Matching

Use `match` with explicit variants over wildcard arms when the enum is under your control. Wildcards hide new variants.

---

## Concurrency

### Actor Model Rules

Each nous is a Tokio actor (Alice Ryhl pattern: actor struct + handle struct).

1. **Bounded channels** - unbounded = memory leak. Default: 32, tune empirically.
2. **Actor owns ALL mutable state.** Handle is a thin `mpsc::Sender`. No `Arc<Mutex<_>>` between them.
3. **Track spawned tasks** - they outlive the actor. Use `JoinHandle` or `CancellationToken`.
4. **Shutdown** - when all handles drop, mpsc closes, actor loop exits. No separate shutdown signal unless cleanup needed.

### Prefer std::sync::Mutex for Short Critical Sections

`std::sync::Mutex` is faster than `tokio::sync::Mutex` for non-contended, non-async operations. Only use `tokio::sync::Mutex` when you must hold the lock across `.await`.

---

## Rust 2024 Edition

### Use Standard Library Types

```rust
// GOOD: standard library (1.80+)
use std::sync::LazyLock;
static CONFIG: LazyLock<Config> = LazyLock::new(|| load_config());

// BAD: external crate for stdlib functionality
use lazy_static::lazy_static;
use once_cell::sync::Lazy;
```

### #[expect(lint)] Over #[allow(lint)]

`#[expect]` warns you when the suppression is no longer needed. `#[allow]` silently persists forever.

```rust
#[expect(dead_code, reason = "used by integration tests only")]
fn internal_helper() { ... }
```

### #[diagnostic::on_unimplemented]

Use on public traits for clear error messages:

```rust
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a valid Aletheia tool",
    label = "this type doesn't implement Tool",
    note = "implement the Tool trait or use #[derive(Tool)]"
)]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(&self, input: Value) -> Result<Value>;
}
```

### Native Async Traits

Use native `async fn` in traits (stable since 1.75). No `async-trait` crate.

### Let Chains

```rust
// 2024 edition
if let Some(agent) = find_agent(id)
    && let Some(session) = agent.active_session()
    && session.is_valid()
{
    process(session);
}
```

### Async Closures

```rust
// 2024 edition - can borrow from environment
let client = reqwest::Client::new();
let fetch = async |url: &str| {
    client.get(url).send().await
};
```

---

## API Design

### Accept impl Into<String>, Return Concrete

```rust
// GOOD: flexible input
pub fn new(name: impl Into<String>) -> Self {
    Self { name: name.into() }
}

// BAD: forces caller to allocate
pub fn new(name: String) -> Self { ... }
```

### Ensure Send + Sync

All types used in async contexts must be `Send + Sync`. Add `static_assertions::assert_impl_all!` for key types:

```rust
static_assertions::assert_impl_all!(NousActor: Send);
static_assertions::assert_impl_all!(SessionStore: Send, Sync);
```

---

## Testing

### Mock at Trait Boundaries

Don't mock internal functions. Define traits at module boundaries and inject test implementations.

```rust
#[cfg(test)]
struct MockStore { ... }

#[cfg(test)]
impl StorageProvider for MockStore {
    // in-memory implementation for testing
}
```

### Use In-Memory CozoDB for Tests

No external services in tests. CozoDB supports in-memory storage - use it.

### Property Tests for Serialization

Every type that implements `Serialize` + `Deserialize` gets a roundtrip property test:

```rust
#[test]
fn config_roundtrip() {
    bolero::check!().with_type::<Config>().for_each(|config| {
        let json = sonic_rs::to_string(config).unwrap();
        let back: Config = sonic_rs::from_str(&json).unwrap();
        assert_eq!(*config, back);
    });
}
```

---

## AI-Specific Anti-Patterns

Things Claude tends to do wrong in Rust. Watch for and correct:

1. **Over-engineering** - wrapper types with no value, trait abstractions with one impl, builders for simple structs. Use `Default` + struct update syntax.
2. **Outdated crate choices** - `lazy_static` (use `LazyLock`), `once_cell` (use `LazyLock`), `async-trait` (use native), `failure` (dead), `reqwest::blocking` in async.
3. **Hallucinated APIs** - verify method signatures against docs.rs. AI invents plausible but nonexistent methods. Always `cargo check`.
4. **Incomplete trait impls** - missing `size_hint` on Iterator, missing `source()` on Error, incomplete Serialize edge cases.
5. **Clone to satisfy borrow checker** - restructure ownership instead.
6. **unwrap() in library code** - use `?` or explicit error handling.
7. **std::sync::Mutex in async** - use tokio::sync::Mutex when holding across `.await`.
8. **Ignoring Send+Sync** - types not Send used across thread boundaries.
9. **CozoDB queries in SQL syntax** - CozoDB uses Datalog, not SQL. Provide reference inline.
10. **Verbose needless code** - run `cargo clippy` after every generation pass.

---

## Logging

`tracing` with structured spans. `#[instrument]` on public functions.

- Spawned tasks MUST propagate spans: `.instrument(span)` or `.in_current_span()`
- Never bare `tokio::spawn()` - always `.instrument()`
- Never hold `span.enter()` across `.await` points

---

## Dependencies

- Prefer std when adequate
- Each new dependency must justify itself
- Pin unstable crates (pre-1.0) to exact versions, wrap in traits
- `serde_yml` is banned (unsound unsafe) - use `serde_yaml` if YAML parsing is needed
- `thiserror` replaced by `snafu` for library crates
- `async-trait` unnecessary - use native async fn in trait

See: docs/TECHNOLOGY.md

---

## Module Boundaries

Same layered import rules as TypeScript. `koina` imports nothing. `taxis` imports only `koina`. Higher layers import lower layers only. No cycles.

Greek naming - modules and crates use Greek terms reflecting their purpose (nous = mind, mneme = memory, hermeneus = interpreter).

See: docs/ARCHITECTURE.md#dependency-rules

---

## Performance Patterns

Known optimization opportunities from TS codebase analysis. Apply when implementing the equivalent Rust module.

- **Prepared statements** - `rusqlite::CachedStatement` for all session queries. TS recompiles SQL every call.
- **SSE broadcast** - Serialize message once, write bytes to all connected clients. Don't serialize per-connection.
- **Lazy JSON deserialization** - `serde_json::value::RawValue` for fields not always needed (`workingState`, `distillationPriming`). Don't parse until accessed.
- **Regex caching** - `LazyLock<RegexSet>` for interaction signal patterns. TS recompiles `RegExp` inside loops.
- **Arena allocation** - `bumpalo` for per-turn transient data (tool results, intermediate parsing). Freed in bulk at turn end.
- **File watching** - `notify` crate for bootstrap files. TS does 11 sync reads per turn; cache and recompute only on change.
- **Batched writes** - Group tool result messages into single SQLite transaction. Don't commit per-message.

---

## Test Execution in Prompts

When working on a specific crate, use **targeted tests during development** and **full suite only as final gate**.

### During Development (after each change)
```bash
cargo test -p <crate-being-modified>
cargo test -p integration-tests
cargo clippy --workspace --all-targets -- -D warnings
```

The clippy pass type-checks the full workspace - it catches cross-crate breakage without running every test.

### Final Gate (once, before creating PR)
```bash
cargo test --workspace
```

This avoids repeated full-suite compilations during iteration while still catching everything before the PR lands.

### After CI Exists
Once `.github/workflows/test.yml` is active, prompts only need targeted tests + clippy. CI handles the full sweep on PR.
