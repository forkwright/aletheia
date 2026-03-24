# krites

Embedded Datalog engine with HNSW vector search, full-text search, and graph algorithms. 55K lines. Vendored from CozoDB with Aletheia-specific facade.

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

## Internal modules (vendored CozoDB)

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
- **Vendored code**: CozoDB internals carry broad `#[expect]` attributes for clippy/pedantic lints. Do not enforce strict linting on vendored modules.
- **Transaction model**: `multi_transaction()` spawns on rayon, communicates via crossbeam channels.

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
