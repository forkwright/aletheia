# krites

## At a glance

Embedded Datalog engine with HNSW vector search and graph algorithms. Depends on eidos. Entry point: `src/lib.rs` (Db, DataValue, FixedRule).

## Depth

Embedded Datalog engine with HNSW vector search, full-text search, and graph algorithms for the Aletheia knowledge graph. 62k lines across 198 files under `src/` (51.3k excluding tests).

## Provenance — read before editing anything here

This crate is substantially derived from **CozoDB** (`cozo-core`), licensed **MPL-2.0**. The attribution of record is [`NOTICE.md`](NOTICE.md), with the license text beside it; do not restate their contents elsewhere, and do not remove them.

Two consequences bind day-to-day work in this crate:

- **The MPL notices stay.** Upstream identifiers were renamed during the migration and the notices were dropped, which is the one thing MPL §3.1 does not permit. That has been repaired. Anything that reads like tidy-up but removes attribution re-creates the defect.
- **`Cargo.toml` deliberately overrides the workspace license** with `AGPL-3.0-or-later AND MPL-2.0`. It is not drift. The derived files, and our modifications to them, stay MPL under file-level copyleft; the AGPL Larger Work is permitted by §3.3.

The naming rule in `docs/HUBS.md` — prefer Krites/Datalog/Fjall over CozoDB — is about **architecture** and explicitly stops short of provenance. Attribution and licensing statements name CozoDB, because they are claims about authorship rather than about how the system is built.

**Verbatim-drift measurement** (retirement program wave 0.3): `scripts/check-krites-verbatim-drift.py` scores any file here against the pinned upstream snapshot at `upstream-snapshot/` (its own `NOTICE.md`). Report-only — see PROMOTION CRITERIA in the script's module docstring before it gates anything.

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
| `storage-fjall` | no | Persistent fjall LSM-tree storage backend |

## Patterns

- **Facade pattern**: public `Db` struct dispatches to `DbInner` enum (Mem or Fjall) for each operation.
- **Error conversion**: internal `InternalError` (rich module-level errors) converted to public `Error` at facade boundary via `convert_internal()`.
- **Query cache**: optional LRU cache normalizes whitespace before key comparison. Attach with `Db::with_cache(capacity)`.
- **Internal engine modules carry per-site lint suppressions with documented reasons.
- **Transaction model**: `multi_transaction()` spawns on rayon, communicates via crossbeam channels.

## Recent substrate notes

- Eval-facing recall scenarios are decoupled from engine internals; keep benchmark and trigger config types at the boundary.
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
