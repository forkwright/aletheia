# Benchmarks

Aletheia uses [criterion.rs](https://github.com/bheisler/criterion.rs) for
microbenchmarks on hot-path functions. Each crate that owns a meaningful
hot path keeps its bench files in `crates/<crate>/benches/` and registers
them in the crate's `Cargo.toml` under `[[bench]]`.

## What gets benched

The bench suite is **not** comprehensive — it tracks the functions that
actually run on every turn or every request. Adding a bench should be
motivated by one of:

- The function is on the request hot path (e.g., JWT validation, rate-limit
  bucket lookup, idempotency check).
- The function is on the per-turn hot path (e.g., token estimation,
  bootstrap section assembly, distillation candidate selection).
- The function is on the per-PR hot path for the bookkeeper (e.g.,
  observation parsing, tag extraction).
- The function is a regression risk (vendored crate replacement, custom
  algorithm with no upstream baseline).

Benches are NOT for:

- One-off computations (e.g., crate startup, test setup).
- I/O-dominated paths (criterion is poorly suited; use `cargo test` with
  release profile and `tracing` spans for those).
- Functions that change frequently — the bench is more cost than signal.

## Running

Run all benches in a single crate:

```bash
cargo bench -p aletheia-koina
cargo bench -p aletheia-graphe
cargo bench -p aletheia-hermeneus
cargo bench -p aletheia-symbolon
cargo bench -p aletheia-episteme
cargo bench -p aletheia-nous
```

Run a specific bench file:

```bash
cargo bench -p aletheia-symbolon --bench jwt
```

Run a specific bench function:

```bash
cargo bench -p aletheia-symbolon --bench jwt -- jwt_validate_round_trip
```

Run all benches with `--quick` for a fast validity check (suitable for CI
and pre-commit):

```bash
cargo bench -p aletheia-symbolon --bench jwt -- --quick
```

## Coverage

| Crate | Bench file | Hot path | Reason |
|---|---|---|---|
| `aletheia-koina` | `benches/ids.rs` | `Ulid::new`, `Uuid::new_v4`, parse round-trip | runs on every session/turn/observation create |
| `aletheia-graphe` | `benches/session_store.rs` | session create, message append, history scan | runs on every turn |
| `aletheia-hermeneus` | `benches/parser.rs` | `Usage::total`, `StopReason::{as_str, from_str}`, `complexity::score_complexity`, `AdaptiveConcurrencyLimiter::{acquire, finish}` | runs on every completion response |
| `aletheia-symbolon` | `benches/jwt.rs` | `JwtManager::{issue_access, validate}` | runs on every authenticated request |
| `aletheia-episteme` | `benches/observation.rs` | `parse_observations`, `extract_tags`, `ObservationType::classify` | runs on every PR scraped by the bookkeeper |
| `aletheia-nous` | `benches/budget.rs` | `CharEstimator::estimate`, `TokenBudget::new` | runs on every system prompt assembly + history truncation |

## Adding a new bench

1. Create `crates/<crate>/benches/<name>.rs`.
2. Add the bench file template:

   ```rust
   //! Microbenchmarks for <crate> <area> hot paths.
   //!
   //! WHY: <one-sentence justification — what runs this code, how often,
   //! and what regression are we guarding against>
   //!
   //! Run: `cargo bench -p aletheia-<crate> --bench <name>`

   #![expect(clippy::expect_used, reason = "bench setup")]

   use std::hint::black_box;
   use criterion::{Criterion, criterion_group, criterion_main};

   fn my_bench(c: &mut Criterion) {
       c.bench_function("my_bench", |b| {
           b.iter(|| {
               let result = some_hot_function(black_box(input));
               black_box(result)
           });
       });
   }

   criterion_group!(benches, my_bench);
   criterion_main!(benches);
   ```

3. Add `criterion = { workspace = true }` to `[dev-dependencies]` if not
   already there.
4. Register the bench in `Cargo.toml`:

   ```toml
   [[bench]]
   name = "<name>"
   harness = false
   ```

5. Verify with `cargo bench -p aletheia-<crate> --bench <name> -- --quick`.
6. Add a row to the coverage table above.

## Baselines and regressions

Criterion writes its baseline data to `target/criterion/`. To compare
against a saved baseline:

```bash
# Save current as baseline
cargo bench -p aletheia-symbolon --bench jwt -- --save-baseline main

# After changes, compare
cargo bench -p aletheia-symbolon --bench jwt -- --baseline main
```

The CI workflow runs `cargo bench --workspace --no-run` to verify that
all bench targets compile, but does not currently track regression
thresholds. That gate is tracked in #2802 follow-up if needed.
