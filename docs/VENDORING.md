# Vendored Dependencies

## What's Vendored

### CozoDB engine (inside mneme)

| Field | Value |
|-------|-------|
| Original project | CozoDB |
| Original crate | cozo-core |
| Version | 0.7.6 |
| Source | https://github.com/cozodb/cozo |
| License | MPL-2.0 |
| Copyright | Copyright 2022-2024 Ziyang Hu and CozoDB contributors |
| Location | `crates/mneme/src/engine/` (behind `mneme-engine` feature gate) |

CozoDB transitively depends on `graph_builder` (0.4.1, MIT, neo4j-labs) via crates.io. It is not vendored on disk.

## License Compliance

Per MPL-2.0 Section 3.1, source files from CozoDB retain their original license. MPL-2.0 is compatible with Aletheia's AGPL-3.0-or-later per Section 3.3 (Secondary License). Copyright headers in absorbed source files are preserved verbatim.

## Modifications from Original

### CozoDB engine

- Storage backends removed: `rocks.rs` (legacy), `sqlite.rs`, `sled.rs`, `tikv.rs`
- Chinese tokenizer removed: `fts/cangjie/`, jieba-rs dependency (~4,000 lines)
- FFI/binding code removed: `DbInstance`, all `*_str` methods
- HTTP fetch utility removed: `fixed_rule/utilities/jlines.rs`, minreq dependency
- CSV reader utility removed: `fixed_rule/utilities/csv.rs`, csv dependency
- Stopwords trimmed to English-only (from 21,885 lines to ~1,303 lines)
- `lib.rs` rewritten: new `Db` facade enum replacing `DbInstance`
- `env_logger` moved to dev-dependencies
- Absorbed into `crates/mneme/src/engine/` as a feature-gated module

### Error architecture (Phase E cleanup)

- Blanket `#[expect]` on `pub mod engine` in `lib.rs` removed
- Per-module snafu error enums: `DataError`, `ParseError`, `QueryError`, `RuntimeError`, `StorageError`, `FtsError`, `FixedRuleError`
- `InternalError` composition enum with `#[snafu(context(false))]` for `?`-based propagation
- Public `Error` type at facade boundary via `convert_internal()`
- `BoxErr`/`DbResult`/`AdhocError` eliminated — all error context preserved
- `bail!`/`miette!`/`ensure!` macros deleted — snafu context selectors throughout
- All unsafe sites documented with SAFETY comments
- Per-submodule `#[expect]` in `engine/mod.rs` with specific lints and reasons
- Dead code removed, unused imports cleaned up

## Upstream Status

### CozoDB

| Field | Value |
|-------|-------|
| Repository | https://github.com/cozodb/cozo |
| Version absorbed | 0.7.6 (cozo-core) |
| Last upstream commit | 2024-12-04 |
| Upstream status | Inactive (no commits since Dec 2024) |

**Relevant issues:**
- **#287** - env_logger in non-dev dependencies. Moved to dev-dependencies in absorption.

No unmerged PRs contain fixes we need. Upstream is inactive - no divergence risk. Future CozoDB development (if any) would need manual review for cherry-pick into the engine module.

## Cleanup Backlog

### Unsafe Sites — Resolved

All 14 unsafe sites (12 `unsafe {}` blocks + 2 `unsafe impl`) now have `// SAFETY:`
comments (P202, verified P302). Each comment explains the precondition, why the
invariant holds at the call site, and what would break if violated.

### Lint Suppressions — Resolved

The blanket `#[expect(...)]` on `pub mod engine` in `lib.rs` has been removed.
Each submodule in `engine/mod.rs` now carries its own `#[expect]` with specific
lints and a reason string. No `#[allow]` blocks remain.

| Lint | Resolution |
|------|-----------|
| `clippy::pedantic` | Per-submodule `#[expect]` — vendored code, cosmetic fixes deferred |
| `clippy::mutable_key_type` | Per-submodule `#[expect]` — `DataValue` hash is structural |
| `clippy::result_large_err` | Per-submodule `#[expect]` — structured error context preserved |
| `clippy::type_complexity` | Per-submodule `#[expect]` — query engine generics are inherent |
| `clippy::too_many_arguments` | Per-submodule `#[expect]` — domain-inherent signatures |
| `private_interfaces` | Per-submodule `#[expect]` — `InternalError` is `pub(crate)` by design |
| `unsafe_code` | Per-submodule `#[expect]` on `data` and `runtime` modules |
| `dead_code` | Removed or `#[cfg(test)]`-gated |
| `unused_imports` | Removed |

### Deferred Unwrap Conversions — Resolved

- **`data/memcmp.rs`:** Write-to-`Vec<u8>` is infallible — retained with SAFETY documentation.
- **`from_shape_ptr` alignment:** Documented with SAFETY comments per P202.
- **`query/ra.rs` store-map lookups:** Retained — infallible by construction, documented.
- **Error enum hierarchy:** Implemented — per-module snafu enums composing into `InternalError`.
