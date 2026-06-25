# Rust Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [MANIFEST/no-deny-toml](#manifestno-deny-toml)
- [MANIFEST/no-license](#manifestno-license)
- [MANIFEST/no-msrv](#manifestno-msrv)
- [MANIFEST/no-rustfmt](#manifestno-rustfmt)
- [MANIFEST/no-workspace-package](#manifestno-workspace-package)
- [MANIFEST/package-workspace-inheritance](#manifestpackage-workspace-inheritance)
- [RUST/allow-not-expect](#rustallow-not-expect)
- [RUST/anyhow-in-lib](#rustanyhow-in-lib)
- [RUST/as-cast](#rustas-cast)
- [RUST/bare-assert](#rustbare-assert)
- [RUST/barrel-reexport](#rustbarrel-reexport)
- [RUST/blanket-allow-limit](#rustblanket-allow-limit)
- [RUST/blocking-in-async](#rustblocking-in-async)
- [RUST/box-dyn-error](#rustbox-dyn-error)
- [RUST/commented-code](#rustcommented-code)
- [RUST/config-deny-unknown-fields](#rustconfig-deny-unknown-fields)
- [RUST/crate-level-denied-allow](#rustcrate-level-denied-allow)
- [RUST/dbg-macro](#rustdbg-macro)
- [RUST/doc-promised-observability](#rustdoc-promised-observability)
- [RUST/empty-match-arm](#rustempty-match-arm)
- [RUST/error-enum-design](#rusterror-enum-design)
- [RUST/expect](#rustexpect)
- [RUST/feature-gate-check](#rustfeature-gate-check)
- [RUST/file-too-long](#rustfile-too-long)
- [RUST/filefilter-custom-arc-not-box](#rustfilefilter-custom-arc-not-box)
- [RUST/format-single-var](#rustformat-single-var)
- [RUST/hardcoded-model](#rusthardcoded-model)
- [RUST/import-order](#rustimport-order)
- [RUST/indexing-slicing](#rustindexing-slicing)
- [RUST/must-use-result-builder](#rustmust-use-result-builder)
- [RUST/no-direct-process-command](#rustno-direct-process-command)
- [RUST/no-tautological-clippy-suppress](#rustno-tautological-clippy-suppress)
- [RUST/non-exhaustive-enum](#rustnon-exhaustive-enum)
- [RUST/prefer-expect-over-allow](#rustprefer-expect-over-allow)
- [RUST/println-in-lib](#rustprintln-in-lib)
- [RUST/pub-visibility](#rustpub-visibility)
- [RUST/rc-in-async](#rustrc-in-async)
- [RUST/return-unit](#rustreturn-unit)
- [RUST/select-cancel-safety](#rustselect-cancel-safety)
- [RUST/silent-error-ok](#rustsilent-error-ok)
- [RUST/silent-wildcard-success](#rustsilent-wildcard-success)
- [RUST/spawn-no-instrument](#rustspawn-no-instrument)
- [RUST/std-mutex-in-async](#ruststd-mutex-in-async)
- [RUST/string-slice](#ruststring-slice)
- [RUST/struct-too-many-fields](#ruststruct-too-many-fields)
- [RUST/test-missing-use-super](#rusttest-missing-use-super)
- [RUST/todo-no-issue](#rusttodo-no-issue)
- [RUST/unwrap](#rustunwrap)
- [WORKFLOW/cargo-without-target-dir-in-worktree](#workflowcargo-without-target-dir-in-worktree)

## `MANIFEST/no-deny-toml` {#manifestno-deny-toml}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `MANIFEST/no-deny-toml`
- See also: `MANIFEST/no-license`

Cargo workspaces must carry a root `deny.toml` for cargo-deny policy. Dependency license and advisory checks need a checked-in policy file so CI and local runs enforce the same supply-chain constraints.

### Examples

**Good:** Keep cargo-deny policy at the workspace root.

```text
Cargo.toml
[workspace]
members = ["crates/*"]

deny.toml
[advisories]
version = 2
```

**Bad:** Define a Cargo workspace without cargo-deny policy.

```text
Cargo.toml
[workspace]
members = ["crates/*"]
```

## `MANIFEST/no-license` {#manifestno-license}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `MANIFEST/no-license`
- See also: `MANIFEST/license-spdx-match`

Workspace Cargo manifests must declare a `license` or `license-file` field in `[workspace.package]` or `[package]`. License metadata keeps compliance and distribution tooling aligned with the repository's actual license.

### Examples

**Good:** Declare the workspace license in Cargo metadata.

```text
[workspace.package]
license = "AGPL-3.0-or-later"
```

**Bad:** Publish a workspace manifest without license metadata.

```text
[workspace]
members = ["crates/*"]
```

## `MANIFEST/no-msrv` {#manifestno-msrv}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `MANIFEST/no-msrv`

Workspace Cargo manifests must declare a `rust-version` MSRV. The declared MSRV tells consumers, CI, and MSRV-aware Cargo dependency resolution which Rust version the workspace supports.

### Examples

**Good:** Declare the workspace MSRV in Cargo metadata.

```text
[workspace.package]
rust-version = "1.85"
```

**Bad:** Publish a workspace manifest without an MSRV.

```text
[workspace]
members = ["crates/*"]
```

## `MANIFEST/no-rustfmt` {#manifestno-rustfmt}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `MANIFEST/no-rustfmt`
- See also: `MANIFEST/no-msrv`

Rust workspaces must keep a minimal repo-local `rustfmt.toml` that declares the edition and avoids style overrides. A shared formatting contract keeps `cargo fmt` output consistent across contributors and automation.

### Examples

**Good:** Keep a minimal rustfmt configuration at the workspace root.

```text
rustfmt.toml
edition = "2024"
```

**Bad:** Publish a workspace without a rustfmt configuration.

```text
Cargo.toml
[workspace]
members = ["crates/*"]
```

## `MANIFEST/no-workspace-package` {#manifestno-workspace-package}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `MANIFEST/no-workspace-package`
- See also: `MANIFEST/no-license`, `MANIFEST/no-msrv`

Root Cargo workspace manifests must declare `[workspace.package]` so shared package metadata such as edition, version, license, and MSRV has one authoritative declaration.

### Examples

**Good:** Declare shared package metadata at the workspace root.

```text
[workspace]
members = ["crates/*"]

[workspace.package]
edition = "2024"
license = "AGPL-3.0-or-later"
```

**Bad:** Publish a Rust workspace without shared package metadata.

```text
[workspace]
members = ["crates/*"]
```

## `MANIFEST/package-workspace-inheritance` {#manifestpackage-workspace-inheritance}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `MANIFEST/package-workspace-inheritance`
- See also: `MANIFEST/no-msrv`, `MANIFEST/no-workspace-package`

Workspace member Cargo manifests must inherit package metadata that `[workspace.package]` already provides. Directly redeclaring version, edition, or rust-version in member crates lets shared metadata drift from the root declaration.

### Examples

**Good:** Inherit shared package metadata from the workspace root.

```text
[package]
version.workspace = true
edition.workspace = true
rust-version.workspace = true
```

**Bad:** Redeclare package metadata that the workspace already owns.

```text
[package]
version = "0.2.0"
edition = "2021"
rust-version = "1.80"
```

## `RUST/allow-not-expect` {#rustallow-not-expect}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/allow-not-expect`
- See also: `RUST/prefer-expect-over-allow`

`#[allow(...)]` suppresses a lint without proving the suppression is still needed. Use `#[expect(..., reason = "...")]` so stale suppressions fail when the lint no longer fires and the reason records the invariant.

### Examples

**Good:** Use an expectation with a reason for a justified lint suppression.

```text
#[expect(clippy::too_many_arguments, reason = "constructor mirrors external schema")]
```

**Bad:** Silence the lint without proving the suppression is still needed.

```text
#[allow(clippy::too_many_arguments)]
```

## `RUST/anyhow-in-lib` {#rustanyhow-in-lib}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/anyhow-in-lib`
- See also: `RUST/box-dyn-error`

Library crates must expose typed errors instead of `anyhow`. `anyhow` erases failure variants, which prevents callers from matching specific failure modes and makes error handling less testable.

### Examples

**Good:** Return a typed library error that callers can match.

```text
pub fn load(path: &Path) -> Result<Config, ConfigError>
```

**Bad:** Erase library errors behind anyhow.

```text
pub fn load(path: &Path) -> anyhow::Result<Config>
```

## `RUST/as-cast` {#rustas-cast}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/as-cast`

`as` casts between numeric types silently truncate, wrap, or lose precision. Use `TryFrom` or `TryInto` with explicit error handling so narrowing conversions are visible and testable.

### Examples

**Good:** Use checked conversion so lossy casts become explicit errors.

```text
let account_id = u32::try_from(raw_id)?;
```

**Bad:** Cast between numeric types without checking truncation.

```text
let account_id = raw_id as u32;
```

## `RUST/bare-assert` {#rustbare-assert}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/bare-assert`
- See also: `RUST/expect`

Assertions in library code must include a descriptive message. Bare assertions only report the failed expression, while a message records the invariant that callers or maintainers need to diagnose the panic.

### Examples

**Good:** Explain the invariant that failed.

```text
assert!(items.len() <= limit, "batch must fit configured limit");
```

**Bad:** Assert without a message.

```text
assert!(items.len() <= limit);
```

## `RUST/barrel-reexport` {#rustbarrel-reexport}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/barrel-reexport`
- See also: `ARCHITECTURE/trait-impl-colocation`

Wildcard re-exports make the public API implicit and fragile. Re-export concrete items so adding a symbol to the source module cannot silently expand the downstream API surface.

### Examples

**Good:** Re-export the public API surface explicitly.

```text
pub use crate::codec::FrameDecoder;
```

**Bad:** Wildcard re-export an entire module.

```text
pub use crate::codec::*;
```

## `RUST/blanket-allow-limit` {#rustblanket-allow-limit}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/blanket-allow-limit`
- See also: `RUST/prefer-expect-over-allow`

Crate roots may have at most 2 crate-level `#![allow(...)]` directives. More than 2 signals accumulated debt that blankets the entire crate and hides lint violations at every call site.

### Examples

**Good:** Use narrow expectations that document the invariant at the suppressed site.

```text
#[expect(clippy::too_many_lines, reason = "parser states are kept together for auditability")]
```

**Bad:** Blanket a crate with repeated allow directives.

```text
#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]
```

## `RUST/blocking-in-async` {#rustblocking-in-async}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/blocking-in-async`
- See also: `RUST/rc-in-async`

Blocking operations inside async functions stall the executor thread and reduce throughput for every task on that runtime. Use async equivalents such as `tokio::fs` and `tokio::time::sleep`, or isolate unavoidable synchronous work with `spawn_blocking`.

### Examples

**Good:** Use async filesystem APIs inside async functions.

```text
let contents = tokio::fs::read_to_string(path).await?;
```

**Bad:** Block the async runtime thread with synchronous filesystem I/O.

```text
let contents = std::fs::read_to_string(path)?;
```

## `RUST/box-dyn-error` {#rustbox-dyn-error}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/box-dyn-error`
- See also: `RUST/expect`

`Box<dyn Error>` must not be used in public library return types. Expose a concrete error enum so callers can match variants, preserve context, and test failure behavior without downcasting.

### Examples

**Good:** Expose a concrete error type callers can match.

```text
pub fn load_config(path: &Path) -> Result<Config, ConfigError>
```

**Bad:** Erase the failure shape behind a dynamic error object.

```text
pub fn load_config(path: &Path) -> Result<Config, Box<dyn Error>>
```

## `RUST/commented-code` {#rustcommented-code}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/commented-code`
- See also: `COMMENTS/journal-shaped`

Commented-out code should be deleted instead of preserved inline. Dead code blocks confuse readers about current intent, and git history already preserves removed implementations.

### Examples

**Good:** Delete dead code and rely on version control for history.

```text
fn parse(input: &str) -> Result<Token, Error> {
    parser::parse(input)
}
```

**Bad:** Leave a stale block of commented-out Rust code in place.

```text
// fn old_parse(input: &str) -> Token {
//     legacy_parse(input)
// }

```

## `RUST/config-deny-unknown-fields` {#rustconfig-deny-unknown-fields}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/config-deny-unknown-fields`

Config types loaded from files, environment, or user input must reject unrecognized fields. Use `#[serde(deny_unknown_fields)]` so misspelled keys fail at parse time instead of silently taking defaults.

### Examples

**Good:** Reject misspelled config keys at parse time.

```text
#[serde(deny_unknown_fields)]
pub struct RetryConfig { pub max_attempts: u32 }
```

**Bad:** Let an unknown config key silently fall back to a default.

```text
#[derive(Deserialize)]
pub struct RetryConfig { pub max_attempts: u32 }
```

## `RUST/crate-level-denied-allow` {#rustcrate-level-denied-allow}

- Severity: `error`
- Scope: `language:rust`
- Enforcer: `RUST/crate-level-denied-allow`
- See also: `RUST/blanket-allow-limit`, `RUST/prefer-expect-over-allow`

Crate-level `#![allow(...)]` attributes must not suppress high-risk lints across an entire module. Replace blanket suppression with per-site `#[expect(..., reason = "...")]` so each exception is justified and stale suppressions fail loudly.

### Examples

**Good:** Use a site-specific expectation with a reason.

```text
#[expect(clippy::unwrap_used, reason = "legacy parser invariant is tracked")]
let field = parts.next().unwrap();
```

**Bad:** Blanket a whole crate or module with a high-risk allow.

```text
#![allow(clippy::unwrap_used)]
```

## `RUST/dbg-macro` {#rustdbg-macro}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/dbg-macro`
- See also: `RUST/println-in-lib`

`dbg!()` is a temporary development tool and must not remain in library code. Use structured tracing for diagnostic output so callers can filter it and avoid leaking internal values to stderr.

### Examples

**Good:** Use structured debug logging that callers can filter.

```text
tracing::debug!(?state, "loaded parser state");
```

**Bad:** Leave temporary debugging output in library code.

```text
dbg!(state);
```

## `RUST/doc-promised-observability` {#rustdoc-promised-observability}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/doc-promised-observability`
- See also: `RUST/spawn-no-instrument`

Rustdoc that promises tracing events, spans, or warnings is an operator contract. The implementation must emit the documented observability signal or remove the false claim from the docs.

### Examples

**Good:** Emit the tracing signal promised by the doc comment.

```text
/// Emits a tracing event when refresh completes.
pub fn refresh() {
    tracing::info!("refresh complete");
}
```

**Bad:** Promise observability in docs without emitting it.

```text
/// Emits a tracing event when refresh completes.
pub fn refresh() {
    rebuild_cache();
}
```

## `RUST/empty-match-arm` {#rustempty-match-arm}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/empty-match-arm`
- See also: `RUST/expect`

Wildcard match arms must not silently discard values with `()`, `{}`, or equivalent empty bodies. Handle the case explicitly or leave a comment that records why ignoring the value is intentional.

### Examples

**Good:** Document why a wildcard arm intentionally does nothing.

```text
match event {
    Event::Tick => refresh(),
    _ => { /* ignored during shutdown */ }
}
```

**Bad:** Discard future enum variants with an empty wildcard arm.

```text
match event {
    Event::Tick => refresh(),
    _ => (),
}
```

## `RUST/error-enum-design` {#rusterror-enum-design}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/error-enum-design`
- See also: `RUST/box-dyn-error`

Public Rust library functions must return concrete error types instead of `Box<dyn Error>`. A named error enum preserves matchable variants, source context, and downstream evolution without forcing callers to downcast.

### Examples

**Good:** Expose a concrete error enum from public library APIs.

```text
pub fn load_config(path: &Path) -> Result<Config, ConfigError>
```

**Bad:** Erase the public failure shape behind a dynamic error object.

```text
pub fn load_config(path: &Path) -> Result<Config, Box<dyn Error>>
```

## `RUST/expect` {#rustexpect}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/expect`
- See also: `RUST/unwrap`

`expect()` panics on `None` or `Err`. Library code must propagate recoverable errors instead of aborting the process; reserve expectation-style panics for static invariants that cannot fail at runtime.

### Examples

**Good:** Return the recoverable failure with context.

```text
let config = load_config(path).context(LoadConfigSnafu { path })?;
```

**Bad:** Abort library code on a caller-recoverable failure.

```text
let config = load_config(path).expect("config exists");
```

## `RUST/feature-gate-check` {#rustfeature-gate-check}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/feature-gate-check`

`#[cfg(feature = "...")]` gates must reference features declared in the nearest Cargo.toml. Undefined feature names silently disable gated code or break intended builds, so cfg names must stay aligned with manifest feature declarations.

### Examples

**Good:** Declare the feature name that cfg-gated Rust code uses.

```text
[features]
postgres = []

#[cfg(feature = "postgres")]
pub fn connect() {}
```

**Bad:** Gate Rust code on a feature name that Cargo.toml does not declare.

```text
#[cfg(feature = "postgress")]
pub fn connect() {}
```

## `RUST/file-too-long` {#rustfile-too-long}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/file-too-long`

Rust files that exceed the configured line limit must be split into smaller modules with focused responsibilities. Long files are harder to review, navigate, and maintain because unrelated concerns accumulate in one surface.

### Examples

**Good:** Split a large Rust surface into focused modules.

```text
mod parser;
mod render;
mod store;
```

**Bad:** Let one Rust file accumulate too many responsibilities.

```text
src/lib.rs: 944 lines
```

## `RUST/filefilter-custom-arc-not-box` {#rustfilefilter-custom-arc-not-box}

- Severity: `error`
- Scope: `language:rust`
- Enforcer: `RUST/filefilter-custom-arc-not-box`

`FileFilter::Custom` predicates must be wrapped with `Arc::new`, not `Box::new`. The custom filter variant is cloned with `LintRule`, and the Arc shape keeps rule construction aligned with the enum contract.

### Examples

**Good:** Wrap custom file filters in Arc so LintRule stays cloneable.

```text
FileFilter::Custom(Arc::new(is_agent_output))
```

**Bad:** Use the old Box wrapper for a custom file filter.

```text
FileFilter::Custom(Box::new(is_agent_output))
```

## `RUST/format-single-var` {#rustformat-single-var}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/format-single-var`
- See also: `RUST/return-unit`

`format!("{}", value)` routes a simple string conversion through the formatting machinery. Use `value.to_string()` for one-value conversions and reserve `format!` for actual templates.

### Examples

**Good:** Convert a single display value directly to a string.

```text
let label = name.to_string();
```

**Bad:** Use format machinery for a single placeholder conversion.

```text
let label = format!("{}", name);
```

## `RUST/hardcoded-model` {#rusthardcoded-model}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/hardcoded-model`

LLM model names must come from configuration or environment, not source literals. Provider model names change frequently, and configuration keeps model selection deployable without code changes.

### Examples

**Good:** Read the model name from configuration.

```text
let model = config.model_name.as_str();
```

**Bad:** Embed a provider model name directly in source code.

```text
let model = "claude-3-opus";
```

## `RUST/import-order` {#rustimport-order}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/import-order`

Rust imports are grouped in dependency order: standard library, external crates, workspace crates, then local modules. Stable grouping keeps dependencies scannable and reduces noisy diffs.

### Examples

**Good:** Group imports as std, external crates, then local modules.

```text
use std::path::Path;

use regex::Regex;

use crate::error::Result;
```

**Bad:** Mix local and std imports.

```text
use crate::error::Result;
use std::path::Path;
```

## `RUST/indexing-slicing` {#rustindexing-slicing}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/indexing-slicing`
- See also: `RUST/string-slice`

Direct indexing and slicing panic when input falls outside the expected bounds. Use checked access such as `.get()` and make the missing-value path explicit.

### Examples

**Good:** Use checked access and handle the missing element explicitly.

```text
let item = items.get(index).ok_or(Error::MissingItem)?;
```

**Bad:** Index directly into a slice and panic on unexpected input.

```text
let item = items[index];
```

## `RUST/must-use-result-builder` {#rustmust-use-result-builder}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/must-use-result-builder`

Public builder entrypoints and builder setters that return `Self` must carry `#[must_use]`. Silently dropped builders hide bugs by discarding partial state before it can be consumed.

### Examples

**Good:** Mark builder entrypoints so dropped builders produce compiler warnings.

```text
#[must_use]
pub fn builder() -> ClientBuilder
```

**Bad:** Return a builder that can be silently discarded.

```text
pub fn builder() -> ClientBuilder
```

## `RUST/no-direct-process-command` {#rustno-direct-process-command}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/no-direct-process-command`

Production command execution must go through the project wrapper so argument assembly, timeouts, stdout and stderr capture, structured errors, and tracing stay centralized. Direct `std::process::Command` is reserved for the command-execution substrate and documented carve-outs.

### Examples

**Good:** Route process execution through the project wrapper.

```text
let output = epitelesis::run(epitelesis::Command::new("git").arg("status"))?;
```

**Bad:** Bypass wrapper-owned timeout, tracing, and error handling.

```text
let output = std::process::Command::new("git").output()?;
```

## `RUST/no-tautological-clippy-suppress` {#rustno-tautological-clippy-suppress}

- Severity: `error`
- Scope: `language:rust`
- Enforcer: `RUST/no-tautological-clippy-suppress`
- See also: `RUST/allow-not-expect`, `RUST/prefer-expect-over-allow`

`#[expect(..., reason = "...")]` reasons must document the invariant that justifies the suppression. Tautological reasons such as "test code" or "unwrap is fine here" restate the suppression instead of explaining why the lint is wrong for that site.

### Examples

**Good:** Document the invariant that makes a lint suppression acceptable.

```text
#[expect(clippy::unwrap_used, reason = "INVARIANT: test fixture always has a root element")]
```

**Bad:** Restate the lint or surrounding context instead of the invariant.

```text
#[expect(clippy::unwrap_used, reason = "test code")]
```

## `RUST/non-exhaustive-enum` {#rustnon-exhaustive-enum}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/non-exhaustive-enum`

Public enums must use `#[non_exhaustive]` unless their variant set is permanently closed. The attribute forces downstream wildcard matches, preserving room to add variants without breaking callers.

### Examples

**Good:** Keep public enums evolvable for downstream crates.

```text
#[non_exhaustive]
pub enum ClientError {
    Timeout,
}
```

**Bad:** Expose a public enum that downstream crates can match exhaustively.

```text
pub enum ClientError {
    Timeout,
}
```

## `RUST/prefer-expect-over-allow` {#rustprefer-expect-over-allow}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/prefer-expect-over-allow`
- See also: `RUST/expect`

`#[expect(...)]` must be used instead of bare `#[allow(...)]` for lint suppressions. Expectations fail loudly when the lint stops firing, and the `reason` field records the invariant that justifies the suppression.

### Examples

**Good:** Use an expectation with an invariant-bearing reason.

```text
#[expect(clippy::too_many_lines, reason = "parser states are kept together for auditability")]
```

**Bad:** Silence a lint without checking whether the suppression still applies.

```text
#[allow(clippy::too_many_lines)]
```

## `RUST/println-in-lib` {#rustprintln-in-lib}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/println-in-lib`

Library code must not write directly to stdout or stderr with console macros. Emit structured logs through `tracing` so callers control filtering, formatting, and collection.

### Examples

**Good:** Emit structured logs through tracing.

```text
tracing::info!(request_id = %request_id, "loaded config");
```

**Bad:** Write unstructured console output from library code.

```text
println!("loaded config");
```

## `RUST/pub-visibility` {#rustpub-visibility}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/pub-visibility`

Rust items should use the narrowest visibility that supports their callers. Bare `pub` expands API surface and creates accidental coupling; prefer `pub(crate)`, `pub(super)`, or a private item unless the item is part of the public contract.

### Examples

**Good:** Keep helper visibility scoped to the callers that need it.

```text
pub(crate) fn parse_config(path: &Path) -> Result<Config>
```

**Bad:** Expose an internal helper as public API without need.

```text
pub fn parse_config(path: &Path) -> Result<Config>
```

## `RUST/rc-in-async` {#rustrc-in-async}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/rc-in-async`
- See also: `RUST/silent-error-ok`

`Rc<T>` must not be used in async library code. Async runtimes may move tasks between threads, so shared state that crosses async boundaries must use `Arc<T>` or another Send-safe owner.

### Examples

**Good:** Use a thread-safe reference count for async tasks.

```text
let shared_state = Arc::clone(&state);
```

**Bad:** Capture non-Send shared state inside async library code.

```text
let shared_state: Rc<State> = Rc::clone(&state);
```

## `RUST/return-unit` {#rustreturn-unit}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/return-unit`

Regular Rust functions should not spell an explicit `-> ()` return type. Unit is the implicit return type, so writing it in ordinary function signatures adds noise without changing behavior.

### Examples

**Good:** Let Rust's implicit unit return type carry the signature.

```text
fn refresh_cache() {
    rebuild();
}
```

**Bad:** Spell an explicit unit return type on a function.

```text
fn refresh_cache() -> () {
    rebuild();
}
```

## `RUST/select-cancel-safety` {#rustselect-cancel-safety}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/select-cancel-safety`
- See also: `RUST/blocking-in-async`

`tokio::select!` drops unfinished branches when another branch completes. Branches must use cancel-safe futures or document the safety invariant so partial reads and writes cannot be lost silently.

### Examples

**Good:** Use a cancel-safe receive operation inside select.

```text
tokio::select! {
    Some(message) = receiver.recv() => handle(message).await?,
}
```

**Bad:** Drop partial progress from a non-cancel-safe read branch.

```text
tokio::select! {
    contents = reader.read_to_string(&mut buffer) => contents?,
}
```

## `RUST/silent-error-ok` {#rustsilent-error-ok}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/silent-error-ok`
- See also: `RUST/unwrap`

Calling `.ok()` on a `Result` must not silently discard an error. Convert to `Option` only when the absence is visible downstream; otherwise handle, log, or propagate the error.

### Examples

**Good:** Handle or propagate the error.

```text
file.flush().context(FlushSnafu)?;
```

**Bad:** Erase the error without recording or handling it.

```text
file.flush().ok();
```

## `RUST/silent-wildcard-success` {#rustsilent-wildcard-success}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/silent-wildcard-success`
- See also: `RUST/silent-error-ok`

Wildcard match arms must not return success without doing visible work. Handle the unhandled state explicitly, return an error, emit a diagnostic, or document the intentional ignore with a structured comment.

### Examples

**Good:** Handle the wildcard case with visible work or a diagnostic.

```text
State::Unknown => return Err(StateError::Unknown),
```

**Bad:** Let the wildcard arm report success without doing work.

```text
_ => Ok(()),
```

## `RUST/spawn-no-instrument` {#rustspawn-no-instrument}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/spawn-no-instrument`

`tokio::spawn` in library code must attach a tracing span with `.instrument(...)`. Spawned tasks otherwise lose parent context, leaving logs and spans orphaned from the work that scheduled them.

### Examples

**Good:** Instrument spawned async work with an explicit tracing span.

```text
let span = tracing::info_span!("worker_task");
tokio::spawn(async move { worker.run().await }.instrument(span));
```

**Bad:** Spawn work without propagating tracing context.

```text
tokio::spawn(async move { worker.run().await; });
```

## `RUST/std-mutex-in-async` {#ruststd-mutex-in-async}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/std-mutex-in-async`
- See also: `RUST/blocking-in-async`

`std::sync::Mutex` blocks the current thread. In async library code, that can starve the executor or deadlock when a guard crosses an await point; use `tokio::sync::Mutex` when async ownership is required.

### Examples

**Good:** Use an async-aware mutex when a lock may cross an await point.

```text
let mut guard = state.lock().await;
```

**Bad:** Import a blocking mutex in async library code.

```text
use std::sync::Mutex;
```

## `RUST/string-slice` {#ruststring-slice}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/string-slice`
- See also: `RUST/unwrap`

String slicing by byte range can panic on invalid UTF-8 boundaries or out-of-bounds indexes. Use `str::get` or iterator APIs so invalid ranges stay explicit and recoverable.

### Examples

**Good:** Use `get` so invalid string boundaries stay recoverable.

```text
let prefix = name.get(..3).ok_or(PrefixError)?;
```

**Bad:** Slice a string by byte range and risk a runtime panic.

```text
let prefix = &name[..3];
```

## `RUST/struct-too-many-fields` {#ruststruct-too-many-fields}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/struct-too-many-fields`

Rust structs with more than the configured field budget are hard to construct, destructure, and reason about. Split unrelated fields into smaller domain structs or introduce a builder so responsibilities stay explicit.

### Examples

**Good:** Group related values into smaller domain structs.

```text
pub struct BuildConfig {
    pub package: PackageConfig,
    pub network: NetworkConfig,
    pub cache: CacheConfig,
}
```

**Bad:** Collect many unrelated fields in one flat struct.

```text
pub struct BuildConfig {
    pub f01: String,
    pub f02: String,
    pub f03: String,
    pub f04: String,
    pub f05: String,
    pub f06: String,
    pub f07: String,
    pub f08: String,
    pub f09: String,
    pub f10: String,
    pub f11: String,
    pub f12: String,
    pub f13: String,
}
```

## `RUST/test-missing-use-super` {#rusttest-missing-use-super}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/test-missing-use-super`

Inline Rust test modules should import the parent module with `use super::*;`. That convention keeps tests close to the code under test without noisy full qualification.

### Examples

**Good:** Import the parent module from an inline test module.

```text
#[cfg(test)]
mod tests {
    use super::*;
}
```

**Bad:** Define an inline test module without importing the parent module.

```text
#[cfg(test)]
mod tests {
    #[test]
    fn covers_behavior() {}
}
```

## `RUST/todo-no-issue` {#rusttodo-no-issue}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/todo-no-issue`

TODO and FIXME comments must include an issue tracker reference in `TODO(#NNN)` or `FIXME(#NNN)` form. Untracked TODOs accumulate as invisible technical debt with no owner or timeline.

### Examples

**Good:** Link the TODO to a tracked issue.

```text
// TODO(#123): remove workaround after parser split
```

**Bad:** Leave a TODO without a tracker reference.

```text
// TODO: remove workaround
```

## `RUST/unwrap` {#rustunwrap}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/unwrap`

Library code must not call `.unwrap()` for recoverable failures. Propagate errors with `?` or convert them into typed errors so callers decide how failure is handled.

### Examples

**Good:** Propagate recoverable failure to the caller.

```text
let config = load_config(path)?;
```

**Bad:** Panic on an error the caller could handle.

```text
let config = load_config(path).unwrap();
```

## `WORKFLOW/cargo-without-target-dir-in-worktree` {#workflowcargo-without-target-dir-in-worktree}

- Severity: `warning`
- Scope: `project:dispatch-worktree`
- Enforcer: `WORKFLOW/cargo-without-target-dir-in-worktree`

Cargo commands in dispatch worktree instructions must set an explicit `CARGO_TARGET_DIR`, intentionally unset an inherited target dir, or run through a wrapper that owns the target-dir decision.

### Examples

**Good:** Pin the target directory for a dispatch worktree.

```text
CARGO_TARGET_DIR=/data/target/wt/team-standards-289 cargo check -p basanos
```

**Bad:** Let cargo inherit a shared target dir inside a worktree prompt.

```text
cargo check -p basanos
```

