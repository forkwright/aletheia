# Roadmap: v1.0 CozoDB Absorption

## Overview

Absorb CozoDB 0.7.6 into Aletheia as `mneme-engine` -- fork the source into workspace crates, strip dead weight, fix safety issues, wire into the mneme layer, deliver hybrid retrieval, migrate error idioms, and tune performance. Six phases, each delivering a verifiable capability that unblocks the next. The end state is a single PR on `feat/mneme-engine` with two new crates, a narrowed public API, and hybrid BM25 + HNSW + graph retrieval executing as native Datalog queries.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Copy + Compile** - Create crate skeletons, copy source, strip backends + cangjie + FFI, pin rayon, get cargo check/clippy clean (completed 2026-03-01)
- [x] **Phase 2: Critical Safety** - Fix minhash_lsh UB, document all unsafe sites, add static_assertions for Send+Sync (completed 2026-03-01)
- [ ] **Phase 3: Wire into mneme** - Connect KnowledgeStore to mneme-engine, fact round-trip, HNSW vector search, spawn_blocking wrappers
- [ ] **Phase 4: Hybrid Retrieval** - BM25 + HNSW + graph join in single Datalog query, RRF fusion, HNSW connectivity validation
- [ ] **Phase 5: Error + Idiom Migration** - miette to snafu, log to tracing, lazy_static to LazyLock, systematic unwrap audit
- [ ] **Phase 6: Performance** - Query timeout via cancellation token, ndarray fused distance computation

## Phase Details

### Phase 1: Copy + Compile
**Goal**: Two new workspace crates (`mneme-engine` and `graph-builder`) exist, compile clean, pass clippy, and expose a narrowed public API -- all dead code stripped
**Depends on**: Nothing (first phase)
**Requirements**: CRATE-01, CRATE-02, CRATE-03, CRATE-04, CRATE-05, STRIP-01, STRIP-02, STRIP-03, STRIP-04, STRIP-05, STRIP-06, STRIP-07, STRIP-08, API-01, API-02, API-03, API-04, API-05, API-06, API-07, API-08, SAFE-04, INTG-06, TEST-01, TEST-05, DOCS-02, DOCS-03
**Success Criteria** (what must be TRUE):
  1. `cargo check --workspace` succeeds with mneme-engine and graph-builder as workspace members
  2. `cargo clippy --workspace` is clean (crate-level `#[expect]` allowed for inherited CozoDB warnings only)
  3. CozoDB's own test suite (`runtime/tests.rs`) passes under mneme-engine
  4. Only `Db::open_rocksdb`, `Db::open_mem`, `Db::run`, `export_relations`, `import_relations`, `register_fixed_rule`, `register_callback`, `multi_transaction`, `DataValue`, `NamedRows`, `ScriptMutability`, `ValidityTs` are public -- everything else is `pub(crate)`
  5. `vendor/cozo/` and `vendor/graph_builder/` are deletable (all source lives in `crates/`)
**Plans**: 3 plans, 3 waves (sequential)

Plans:
- [x] 01-01: Scaffold crates and copy source verbatim (Wave 1)
- [x] 01-02: Strip dead code and rewrite lib.rs (Wave 2)
- [x] 01-03: Tests, clippy clean, and documentation (Wave 3)

### Phase 2: Critical Safety
**Goal**: All known undefined behavior is eliminated and every remaining unsafe site is documented with a SAFETY comment
**Depends on**: Phase 1
**Requirements**: SAFE-01, SAFE-02, SAFE-03, SAFE-05
**Success Criteria** (what must be TRUE):
  1. `minhash_lsh.rs:310` uses `bytemuck::try_cast_slice` instead of raw pointer cast -- no UB on unaligned input
  2. Every `unsafe` block in mneme-engine and graph-builder has a `// SAFETY:` comment explaining the invariant
  3. `static_assertions::assert_impl_all!(Db<MemStorage>: Send, Sync)` and equivalent for `Db<RocksDbStorage>` compile
  4. `newrocks.rs` unsafe `Sync` impl has documented safety justification referencing RocksDB's thread-safety guarantees
**Plans**: 2 plans, 1 wave (parallel)

Plans:
- [x] 02-01: Fix minhash_lsh.rs UB with bytemuck, static_assertions, newrocks SAFETY (Wave 1)
- [ ] 02-02: SAFETY comment audit across both crates (Wave 1)

### Phase 3: Wire into mneme
**Goal**: KnowledgeStore uses mneme-engine for storage -- facts round-trip through insert+query, HNSW vector search returns nearest neighbors, graph algorithms run safely from async context
**Depends on**: Phase 2
**Requirements**: INTG-01, INTG-02, INTG-03, INTG-04, INTG-05, TEST-02, TEST-03
**Success Criteria** (what must be TRUE):
  1. Integration test creates an in-memory DB, inserts a fact with `Db::run`, queries it back, and gets the same data
  2. Integration test inserts vectors, runs HNSW kNN search, and retrieves the correct nearest neighbors
  3. All graph algorithm calls from KnowledgeStore go through `spawn_blocking` -- no rayon+Tokio deadlock possible
  4. Schema version is tracked in the mneme wrapper and queryable
**Plans**: TBD

Plans:
- [ ] 03-01: TBD
- [ ] 03-02: TBD

### Phase 4: Hybrid Retrieval
**Goal**: Single Datalog query combines BM25 full-text search, HNSW vector similarity, and graph joins with RRF fusion -- the unique capability that justifies keeping FTS
**Depends on**: Phase 3
**Requirements**: RETR-01, RETR-02, RETR-03, RETR-04, TEST-04, TEST-06, PERF-01
**Success Criteria** (what must be TRUE):
  1. BM25 full-text search executes as a Datalog query and returns ranked results
  2. HNSW vector similarity search executes as a Datalog query and returns nearest neighbors with scores
  3. A single Datalog query combines BM25 + HNSW + graph join, with RRF merging results inside the engine
  4. End-to-end integration test: insert documents with text+vectors+relations, run hybrid query, verify fused ranking
  5. HNSW connectivity verification test confirms less than 5% recall degradation after N delete+reinsert cycles
**Plans**: TBD

Plans:
- [ ] 04-01: TBD
- [ ] 04-02: TBD

### Phase 5: Error + Idiom Migration
**Goal**: Absorbed CozoDB code follows Aletheia conventions -- snafu errors, tracing instrumentation, LazyLock statics, and unwraps in public-reachable paths replaced with typed errors
**Depends on**: Phase 4
**Requirements**: IDIOM-01, IDIOM-02, IDIOM-03, IDIOM-04, IDIOM-05, DOCS-01
**Success Criteria** (what must be TRUE):
  1. Zero `use miette` or `miette::` imports remain -- all error types use snafu
  2. Zero `log::` or `use log` imports remain -- all logging uses tracing macros
  3. Zero `lazy_static!` invocations remain -- all replaced with `LazyLock`
  4. Unwraps on code paths reachable from the 8 public API methods return typed errors instead of panicking
  5. ABSORPTION.md documents lines removed (before/after), unsafe sites carried, unwraps remaining, and cleanup backlog
**Plans**: TBD

Plans:
- [ ] 05-01: TBD
- [ ] 05-02: TBD

### Phase 6: Performance
**Goal**: Query execution is cancellable via timeout and vector distance computation uses fused ndarray operations
**Depends on**: Phase 5
**Requirements**: PERF-02, PERF-03
**Success Criteria** (what must be TRUE):
  1. A long-running Datalog query can be cancelled by a timeout token, returning a typed error instead of running forever
  2. HNSW distance computation uses ndarray fused operations (`Zip::from(...).apply(...)`) instead of element-wise loops
**Plans**: TBD

Plans:
- [ ] 06-01: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5 -> 6

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Copy + Compile | 3/3 | Complete   | 2026-03-01 |
| 2. Critical Safety | 2/2 | Complete   | 2026-03-01 |
| 3. Wire into mneme | 0/2 | Not started | - |
| 4. Hybrid Retrieval | 0/2 | Not started | - |
| 5. Error + Idiom Migration | 0/2 | Not started | - |
| 6. Performance | 0/1 | Not started | - |
