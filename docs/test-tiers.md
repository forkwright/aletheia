# Test tiers

Aletheia uses Cargo feature flags to organize tests into tiers that balance
coverage against build time and resource requirements.

## Tiers

| Tier | Feature flag | What it enables | Approx. tests |
|------|-------------|-----------------|---------------|
| **default** | *(none)* | Full default-feature-unified workspace tests (includes engine and ML via feature unification — see NOTE below) | ~5,400 |
| **test-core** | `--features test-core` | Storage engine tests (Datalog, HNSW, fjall, knowledge store CRUD) | ~5,435 |
| **test-full** | `--features test-full` | ML embedding tests (candle model loading, vector generation) - includes `online-tests` | ~5,435 |
| **all** | `--all-features` | Provider subprocess adapters, computer-use, bookkeeper, z3, other optional features | ~5,475 |

Each tier is a strict superset of the previous: `test-full` implies `test-core`,
which adds to the default set.

Feature scope matters. `cargo test --workspace --all-features` enables every
workspace member's feature set. `cargo test -p aletheia --all-features` enables
only the `aletheia` package features, plus dependency features wired through
its passthroughs.

### Online-only tests

A small set of tests call `CandelProvider::new`, which downloads model weights
from `huggingface.co` at test time. macOS GitHub runners intermittently fail
DNS lookup for HuggingFace, so these tests are gated behind a dedicated
`online-tests` feature (see #3683) and **excluded** from the default PR gate.

- `--features test-core`: fast deterministic gate, no external network.
- `--features test-core,online-tests`: adds the HuggingFace-network candle
  tests. The Online Tests workflow runs this path through `test-full` on its
  schedule, on manual dispatch, and on labeled PRs.
- `--features test-full`: everything, including `online-tests`.

## Usage

```bash
# Developer workflow: fast feedback loop
cargo test --workspace

# CI minimum: includes storage/engine integration tests and JUnit output
cargo nextest run --profile ci --workspace --features test-core

# Full suite: everything including ML tests (needs ~16 GB RAM)
cargo test --workspace --features test-full
```

## How it works

Workspace crates with gated test dependencies define `test-core = [...]` and
`test-full = [...]` features. Crates with no gated tests may omit the tier
features or leave them empty. Crates with storage-dependent tests wire
`test-core` to their engine features (e.g.,
`test-core = ["mneme-engine", "hnsw_rs", "storage-fjall"]`). If `test-core` is
non-empty, `test-full` must include `test-core`; `scripts/check-test-tier-features.py`
enforces that relationship. The root `aletheia` crate propagates features down
to dependencies.

NOTE: The `aletheia` binary's default features (`recall`, `embed-candle`,
`storage-fjall`) enable mneme-engine and embed-candle for all workspace members
via Cargo feature unification. This means `cargo test --workspace` (default
tier) already runs engine and ML tests. The tier distinction matters most for
per-crate testing (`cargo test -p <crate> --features test-core`).

The `--features test-core` flag on `cargo test --workspace` activates matching
workspace member features simultaneously, which is why crates with gated tests
must keep the tier names consistent.

## CI configuration

The PR gate (`.github/workflows/gate-attestation.yml`) first validates test-tier
feature wiring with `scripts/check-test-tier-features.py` and release feature
policy/docs freshness with `scripts/release-feature-policy.py`, then enforces the
**test-core** tier with
`cargo nextest run --profile ci --workspace --features test-core`, after
CI-exact fmt and clippy. The `ci` nextest profile writes JUnit output to
`target/nextest/ci/junit.xml`; the gate uploads that file and nextest logs when
tests fail. The release pipeline
(`.github/workflows/release.yml`) additionally runs
`cargo test --workspace --exclude proskenion` and a generated `feature-check`
matrix from `cargo metadata`. The generator checks every workspace feature
except exclusions documented in `scripts/release-feature-policy.toml`, and it
validates this document's feature table before emitting the release matrices.

The Online Tests workflow exercises `test-full` on its schedule, by manual
dispatch, and for PRs labeled `online-tests`, `test-full`, or
`release-blocking`. It also uses the `ci` nextest profile and uploads nextest
failure artifacts. The default PR gate remains the deterministic `test-core`
tier.

## Adding gated tests

If a new test depends on the storage engine or Datalog:

1. Gate the test module with `#![cfg(feature = "engine-tests")]` (or the
   crate's equivalent gate)
2. Ensure the crate's `test-core` feature enables the required dependency
3. Verify: `cargo test -p <crate> --features test-core`
