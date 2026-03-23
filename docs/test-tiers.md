# Test Tiers

Aletheia uses Cargo feature flags to organize tests into tiers that balance
coverage against build time and resource requirements.

## Tiers

| Tier | Feature flag | What it enables | Approx. tests |
|------|-------------|-----------------|---------------|
| **default** | *(none)* | Pure-logic unit tests, config validation, type invariants | ~5,400 |
| **test-core** | `--features test-core` | Storage engine tests (Datalog, HNSW, fjall, knowledge store CRUD) | ~5,435 |
| **test-full** | `--features test-full` | ML embedding tests (candle model loading, vector generation) | ~5,435 |
| **all** | `--all-features` | + local-llm, computer-use, other optional features | ~5,475 |

Each tier is a strict superset of the previous: `test-full` implies `test-core`,
which adds to the default set.

## Usage

```bash
# Developer workflow: fast feedback loop
cargo test --workspace

# CI minimum: includes storage/engine integration tests
cargo test --workspace --features test-core

# Full suite: everything including ML tests (needs ~16 GB RAM)
cargo test --workspace --features test-full
```

## How it works

Every crate in the workspace defines `test-core = [...]` and `test-full = [...]`
features. Crates with no gated tests leave these empty. Crates with storage-
dependent tests wire `test-core` to their engine features (e.g.,
`test-core = ["mneme-engine", "hnsw_rs", "storage-fjall"]`). The root `aletheia`
crate propagates features down to dependencies.

NOTE: The `aletheia` binary's default features (`recall`, `embed-candle`,
`storage-fjall`) enable mneme-engine and embed-candle for all workspace members
via Cargo feature unification. This means `cargo test --workspace` (default
tier) already runs engine and ML tests. The tier distinction matters most for
per-crate testing (`cargo test -p <crate> --features test-core`).

The `--features test-core` flag on `cargo test --workspace` activates the
feature in every workspace member simultaneously, which is why every crate
must declare the feature even if empty.

## CI configuration

The sharded test workflow (`.github/workflows/test-sharded.yml`) runs with
`--features test-core` by default. The feature-isolation matrix in `rust.yml`
verifies that `test-core` compiles cleanly.

## Adding gated tests

If a new test depends on the storage engine or Datalog:

1. Gate the test module with `#![cfg(feature = "engine-tests")]` (or the
   crate's equivalent gate)
2. Ensure the crate's `test-core` feature enables the required dependency
3. Verify: `cargo test -p <crate> --features test-core`
