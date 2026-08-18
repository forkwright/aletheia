# krites

## At a glance

Embedded Datalog engine with HNSW vector search and graph algorithms. Depends on eidos. Entry point: `src/lib.rs` (Db, DataValue, FixedRule).

## Depth

Embedded Datalog engine with HNSW vector search, full-text search, and graph algorithms for the Aletheia knowledge graph. 62k lines across 198 files under `src/` (51.3k excluding tests).

## Provenance — read before editing anything here

This crate is substantially derived from **CozoDB** (`cozo-core`), licensed **MPL-2.0**. The attribution of record is [`NOTICE.md`](NOTICE.md), with the license text beside it; do not restate their contents elsewhere, and do not remove them.

Two consequences bind day-to-day work in this crate:

- **The MPL notices stay, per file.** Upstream identifiers were renamed during the migration and the notices were dropped, which is the one thing MPL §3.1 does not permit. That has been repaired. Note what "per file" rules out: this crate's `NOTICE.md` recording a file as derived, however accurately, does not substitute for the notice that file is itself required to carry — §3.1 binds the Source Code Form, not the distribution around it. `datalog.pest` was the live example, byte-identical to upstream below a header that had been replaced with a one-line description while the ledger honestly reported it at 99.6%. Anything that reads like tidy-up but removes attribution re-creates the defect. The notice is now rendered into every `derived`/`dual` file from the ledger by `scripts/measure-krites-provenance.py` and gated by `check-krites-provenance.py`, so removing one fails the build rather than going unnoticed — never hand-write or hand-edit the block. A `sovereign` file must carry no notice at all: it claims no CozoDB lineage, so stamping one there asserts an MPL obligation over aletheia's own work, and a `dual` → `sovereign` transition takes the block back out.
  **The generated block is excluded from every verbatim measurement, deliberately.** `verbatim_pct` is matched-lines over the file's own non-blank lines, so a five-line header on 142 derived files would move the de-derivation program's central metric on every one of them while nothing about any file's derivation changed — measured, the mean falls 44.28% → 42.46% and `fts/README.md` reads 44.4% instead of 100.0%. A number that moves without the underlying work is the failure this ledger exists to end, so `krites_provenance_lib.strip_generated_notice` removes the block before either instrument (`verbatim_pct` and the drift metric's `eligible_lines`) counts a line. Anything that adds a third measurement path must call it too.
- **`Cargo.toml` deliberately overrides the workspace license** with `AGPL-3.0-or-later AND MPL-2.0`. It is not drift. The derived files, and our modifications to them, stay MPL under file-level copyleft; the AGPL Larger Work is permitted by §3.3.

The naming rule in `docs/HUBS.md` — prefer Krites/Datalog/Fjall over CozoDB — is about **architecture** and explicitly stops short of provenance. Attribution and licensing statements name CozoDB, because they are claims about authorship rather than about how the system is built.

**Verbatim-drift measurement**: `scripts/check-krites-verbatim-drift.py` scores any file here against the pinned upstream snapshot at `upstream-snapshot/` (its own `NOTICE.md`). The full report is informational, but `--strict` **gates**, on one condition: a row that is `sovereign` AND records `replaced_upstream_path = "none"` AND scores above the calibrated threshold. A `derived` row scoring high is the metric working, and never fails. If it fires on your file, record what the file replaced in `SOVEREIGN_VERIFY_MAP` and regenerate — the row then carries a measured figure instead of an asserted `0.0`. Do not waive it.

## Clean-room rewrites — which siblings you may read

Most of this crate is `derived`, so the sibling that best demonstrates a local convention is usually the sibling doing the same job — exactly the expression a rewrite exists to stop carrying forward. "Match the surrounding crate" and "clean-room" are in real tension here, and the line that makes both workable is what you take from the sibling:

- **Mechanical conventions** — error type, lint attributes, module layout, naming, test shape — may come from any sibling, `derived` included.
- **The shape of the same algorithm** — its control flow, its data structures, the order it does things in — may come only from a `sovereign` one.

Every rewrite records what it read. `PROVENANCE.toml`'s `consulted` column lists the source paths open while writing (`[]` for none), and CI reads each one's own ledger status: `from_spec` means every consulted path was `sovereign`; `from_spec_derived_siblings` means at least one was not. Record both with one command:

```
scripts/krites-provenance-transition.py --set-method from_spec_derived_siblings \
    --evidence '#NNNN' --consulted fts/tokenizer/remove_long.rs,fts/tokenizer/stemmer.rs \
    fts/tokenizer/<your rewrite>.rs
```

A truthful weaker method always beats a false stronger one, and that command is the whole cost of downgrading. What the check cannot see is whether the list is complete: nothing observes what you opened, so a path you leave out reads exactly like a path you never read. The list is worth only the care taken writing it.

**Calibration — for a `TokenStream` filter, roughly 9% verbatim is the interface-forced floor.** The trait dictates the `transform` signature, the `token`/`token_mut` bodies delegate, and the struct declaration and braces are fixed; none of that is expressive. A filter rewrite scoring near there is not carrying residual derivation, and driving toward 0% chases something the trait makes unreachable. Measure after writing, never before — editing toward a number is how a transliteration gets tuned under a threshold instead of rewritten.

## Derived artifacts — never hand-merge, always recompute

`PROVENANCE.toml`, `NOTICE.md`, and the three `module-dag` variants are whole-file
functions of the source tree. Two branches that touch this crate conflict in all of
them whether or not their code overlaps.

`.gitattributes` marks them `-merge`, so a merge leaves them at our side wholesale and
declares the conflict rather than interleaving markers. Resolve with:

```
scripts/regen-krites-artifacts.sh   # then stage
```

Two things that ordering protects, both learned the hard way:

- **`cargo fmt` moves `verbatim_pct`**, which is computed from file bytes. Regenerating
  before formatting records figures for a tree about to change, and the provenance
  checker then fails on a mismatch that reads like a provenance problem and is not.
- **A ledger carrying conflict markers cannot be parsed**, and regenerating against one
  used to rewrite every `dual`/`sovereign` row as `derived` with its soak window zeroed.
  Producing no markers removes that trap rather than guarding it twice.

`CAPABILITY_MATRIX.toml` is NOT in this set. It is hand-maintained and only checked, so a
conflict there needs a human deciding which rows are right.

## Read first

1. `src/lib.rs`: Public Db facade, DbInner dispatch, storage backend selection
2. `src/error.rs`: Public Error (Engine, QueryKilled, Parse, Storage) + InternalError conversion
3. `src/query_cache.rs`: LRU query cache with whitespace normalization and hit/miss metrics
4. `src/storage/mod.rs`: Storage and StoreTx traits (backend abstraction)
5. `src/data/value.rs`: DataValue enum (the core data representation)

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `Db` | `lib.rs` | Public engine facade: `open_mem()`, `open_fjall()`, `run()`, `with_cache()` |
| `NamedRows` | `runtime/db.rs` | Query result: column headers + row data |
| `DataValue` | `data/value.rs` | Core value type: Null, Bool, Num, Str, Bytes, List, Json, Vector, Validity |
| `Vector` | `data/value.rs` | Typed vector: F32, F64 (for HNSW embeddings) |
| `QueryCache` | `query_cache.rs` | LRU cache with whitespace-normalized keys and hit/miss counters |
| `QueryCacheStats` | `query_cache.rs` | Hit/miss/size snapshot for observability |
| `MultiTransaction` | `lib.rs` | Channel-based multi-statement transaction handle |
| `FixedRule` | `fixed_rule/mod.rs` | Trait for custom graph algorithms (PageRank, community detection) |
| `Storage` | `storage/mod.rs` | Trait: backend lifecycle (open, transaction creation) |
| `StoreTx` | `storage/mod.rs` | Trait: key-value operations within a transaction |
| `MemStorage` | `storage/mem.rs` | In-memory storage backend (tests, ephemeral databases) |
| `FjallStorage` | `storage/fjall_backend.rs` | Persistent LSM-tree backend via fjall (requires `storage-fjall`) |
| `ScriptMutability` | `runtime/db.rs` | Enum: Mutable, Immutable (query execution mode) |
| `Poison` | `runtime/db.rs` | Cancellation token for killing long-running queries |

## Internal modules

| Module | Purpose |
|--------|---------|
| `data/` | DataValue, expressions, relations, column types |
| `parse/` | Datalog parser (pest grammar at `src/datalog.pest`) |
| `query/` | Query planner and execution engine |
| `runtime/` | Database core, relation management, callbacks |
| `fixed_rule/` | Built-in graph algorithms (PageRank, shortest path, community) |
| `fts/` | Full-text search tokenizer and indexing |
| `storage/` | Storage backend trait + mem/fjall implementations |

## Feature flags

| Feature | Default | Purpose |
|---------|---------|---------|
| `graph-algo` | yes | Graph algorithms (PageRank, community detection, shortest path) |
| `async` | yes | Async query surface |
| `hot-reload` | yes | Config hot-reload via `notify` + `arc-swap` |
| `storage-fjall` | no | Persistent fjall LSM-tree storage backend |
| `test-core` | no | Storage/engine test tier; implies `storage-fjall` |
| `test-full` | no | ML/embedding test tier; implies `test-core` |

`default = ["graph-algo", "async", "hot-reload"]`.

Land-dark selectors (`krites_sovereign_*`) appear here only while a sovereign rewrite soaks beside
the derived module it replaces. They are off by default, they are removed when the derived copy is
retired, and every one that exists must carry a row in `docs/FEATURE-FLAGS.md` — `release-feature-policy`
fails the gate otherwise.

## Patterns

- **Facade pattern**: public `Db` struct dispatches to `DbInner` enum (Mem or Fjall) for each operation.
- **Error conversion**: internal `InternalError` (rich module-level errors) converted to public `Error` at facade boundary via `convert_internal()`.
- **Query cache**: optional LRU cache normalizes whitespace before key comparison. Attach with `Db::with_cache(capacity)`.
- **Internal engine modules carry per-site lint suppressions with documented reasons.
- **Transaction model**: `multi_transaction()` spawns on rayon, communicates via crossbeam channels.

## Traps specific to this crate

- **Scope clippy to the workspace, not to `-p krites`** (#6633). A scoped `cargo clippy -p krites`
  reports dead-code failures that do not exist under `--workspace`, because the scoped build compiles
  a different set of features and names an innocent file. The in-tree `pre-push` hook defaults to the
  scoped form and calls itself a 95% pre-filter; the missing 5% is a false failure that reads as a
  real one.
- **`cargo fmt` alone moves `verbatim_pct`.** The provenance metric counts non-blank source lines
  shared with the upstream snapshot, so a pure reformat changes a published figure in
  `PROVENANCE.toml` and `NOTICE.md`. Regenerate with `scripts/measure-krites-provenance.py` after any
  formatting change rather than hand-editing the ledger, whose header forbids hand-edits for exactly
  this reason.
- Eval-facing recall scenarios are decoupled from engine internals; keep benchmark and trigger config
  types at the boundary.
- `question_timeout`, ISO-8601 helpers, and `TriggerConfig` support scenario truth without embedding test policy in the engine.

## Common tasks

| Task | Where |
|------|-------|
| Add storage backend | New module in `src/storage/`, implement Storage + StoreTx, add DbInner variant |
| Register graph algorithm | Implement FixedRule trait, call `db.register_fixed_rule()` |
| Modify query cache | `src/query_cache.rs` (QueryCache struct) |
| Add Datalog built-in | `src/fixed_rule/` (new rule module) |
| Update pest grammar | `src/datalog.pest` + `src/parse/` |

## Dependencies

Uses: eidos, rayon, crossbeam, ndarray, pest, serde, snafu, smallvec, fjall (optional)
Used by: mneme (facade re-export), episteme (optional)
