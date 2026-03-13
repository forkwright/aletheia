# ADR-001: snafu for error handling

## Status

Accepted

## Context

Aletheia is a multi-crate Rust workspace. Each crate defines its own error type that
callers can match on, inspect, and propagate. The Rust ecosystem offers several
approaches: `snafu`, `thiserror`, `anyhow`, and hand-rolled `std::error::Error`
impls.

The most popular choice in the broader Rust ecosystem is `thiserror` (~100M downloads
vs `snafu`'s ~25M). New contributors and AI coding agents default to it. Without an
authoritative record, the decision gets relitigated on every new crate. This ADR
closes that.

The codebase currently has 752 uses of `snafu` patterns (`use snafu`, `#[derive(Snafu)]`,
`.context()`, `ensure!`) across 14 library crates. `thiserror` has zero occurrences.

## Decision

**All library crates use `snafu` for error types. `anyhow` is permitted only in the
binary entry point (`crates/aletheia/`) and in test helpers.**

### Pattern

Every library crate has `crates/<name>/src/error.rs`:

```rust
use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum Error {
    #[snafu(display("failed to read config from {}", path.display()))]
    ReadConfig {
        path: std::path::PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("failed to parse YAML config at {}: {reason}", path.display()))]
    ParseYaml {
        path: std::path::PathBuf,
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
```

Error propagation at every call site uses `.context()` with the generated selector:

```rust
use snafu::ResultExt;

fn load_config(path: &Path) -> Result<Config> {
    let contents = std::fs::read_to_string(path)
        .context(ReadConfigSnafu { path: path.to_owned() })?;
    let config: Config = serde_yaml::from_str(&contents)
        .context(ParseYamlSnafu { path: path.to_owned(), reason: "invalid YAML" })?;
    Ok(config)
}
```

### Inline validation uses `ensure!`

```rust
use snafu::ensure;

fn set_timeout(ms: u64) -> Result<()> {
    ensure!(ms > 0, InvalidTimeoutSnafu { ms });
    // ...
    Ok(())
}
```

### Crate boundary convention

- `source` field: internal error, walk the chain (wraps a lower-layer error)
- `error` field: external error, stop walking (the external type is opaque)

```rust
// Internal: chain walks through this
RecallSearch {
    source: aletheia_mneme::error::Error,  // walk into mneme's error
    #[snafu(implicit)
    location: snafu::Location,
},

// External: chain stops here
ParseResponse {
    source: serde_json::Error,  // external crate, stop walking
    #[snafu(implicit)]
    location: snafu::Location,
},
```

### anyhow is permitted in the binary and tests

`crates/aletheia/` is an executable entry point. Its `main()` needs to print a
friendly error and exit, not expose a matchable type to callers. `anyhow` is ideal
there: rich context chains, good `Display`, zero boilerplate. It is also acceptable in
test helpers where error matching is not the goal.

## Consequences

**Positive:**

- **Explicit context at every conversion site.** `.context(ReadConfigSnafu { path })` forces
  the author to name the failure mode and attach relevant data. `thiserror`'s `#[from]`
  converts silently; the conversion point carries no context and no location.

- **`Location` tracking gives a virtual stack trace.** `#[snafu(implicit)]` on every
  variant captures `file!()`, `line!()`, and `column!()` at the `.context()` call, not
  inside the library that originated the error. This pinpoints the conversion site,
  which is usually where the missing context lives.

- **Callers can match precisely.** Each variant is a distinct type. Middleware,
  retry logic, and HTTP mappers can `match` on `RateLimited` vs `AuthFailed` vs
  `ApiRequest` without parsing strings.

- **`ensure!` consolidates guard clauses.** Inline precondition checks produce a
  typed error variant without a separate `if/return` block.

- **`#[non_exhaustive]` on the enum.** New variants don't break downstream `match`
  arms. Already enforced by the lint baseline.

**Negative:**

- **Verbosity.** Each variant needs a struct body with named fields. `thiserror` is
  more terse. This is a deliberate trade: the verbosity is the documentation.

- **Selector name friction.** snafu generates `ReadConfigSnafu` from `ReadConfig`.
  The `Snafu` suffix is mechanical but unfamiliar to newcomers.

- **Two patterns in flight.** `crates/aletheia/` uses `anyhow`, library crates use
  `snafu`. The boundary is clear (binary vs library) but requires discipline.

## Alternatives considered

### thiserror

`thiserror` is more popular and more terse:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to read config from {path}")]
    ReadConfig {
        path: String,
        #[from]
        source: std::io::Error,
    },
}
```

**Why rejected:**

1. `#[from]` performs the conversion automatically wherever `std::io::Error` appears,
   with no context attached. The call site contributes nothing; you get the IO error
   but not which file, which operation, or which layer triggered it.

2. No location tracking. Errors surface where they are created (inside the low-level
   library), not where they cross the crate boundary that matters to the operator.

3. `#[from]` makes it tempting to collapse all IO errors into one variant, hiding
   the semantic differences between "config not found" and "config unreadable" and
   "config directory missing".

4. With `thiserror`, two different failure modes that happen to share the same source
   type (`std::io::Error`) cannot both use `#[from]`; one must fall back to manual
   `impl From<_>`. This creates inconsistency within the same enum.

### anyhow (everywhere)

`anyhow` erases error types entirely. It is excellent for applications that only need
to report errors to a human, but it is wrong for libraries:

1. Callers cannot match on `anyhow::Error`. Retry logic, circuit breakers, and HTTP
   mappers must parse `Display` strings, which is fragile and untestable.

2. `anyhow` errors are not `Send + Sync` by default in all configurations.

3. It signals "I do not care what this error is," which is appropriate for `main()`,
   not for a library function that a caller might want to handle.

### Hand-rolled `impl std::error::Error`

Correct and zero-dependency but high maintenance. Every new variant requires manual
`Display`, `Error::source()`, and often `From` impls. `snafu` generates all of this
from the derive macro. There is no reason to do it by hand.

### miette

`miette` is excellent for *user-facing* diagnostics with rich terminal output. It is
used in this codebase for exactly that: CLI error display. It is not a replacement for
library error types; it wraps them for presentation.

## Crate error inventory

| Crate | Error type | Notes |
|-------|-----------|-------|
| `koina` | `error::Error` | Base layer, no workspace deps |
| `taxis` | `error::Error` | Config cascade errors |
| `mneme` | `error::Error` | Session store, graph, FTS |
| `hermeneus` | `error::Error` | LLM provider errors |
| `organon` | `error::Error` | Tool registry and execution |
| `nous` | `error::Error` | Actor pipeline errors |
| `dianoia` | `error::Error` | Planning errors |
| `melete` | `error::Error` | Distillation errors |
| `agora` | `error::Error` | HTTP API layer |
| `daemon` | `error::Error` | Background maintenance |
| `pylon` | `error::Error` | Network/TLS |
| `symbolon` | `error::Error` | Auth/tokens |
| `thesauros` | `error::Error` | Embeddings |
| `eval` | `error::Error` | Evaluation harness |
| `aletheia` (binary) | `anyhow::Error` | CLI entry point only |

## References

- [snafu docs](https://docs.rs/snafu): context selectors, Location, ensure!
- [GreptimeDB error handling](https://github.com/GreptimeTeam/greptide-db): the pattern this codebase follows
- `standards/RUST.md`: Error Handling section
- `standards/RUST.md`: Error Handling rules (full Rust standards)
- `crates/nous/src/error.rs`: canonical example with Location tracking
- `crates/taxis/src/error.rs`: example with PathBuf fields
