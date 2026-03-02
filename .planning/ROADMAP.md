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
- [x] **Phase 3: Wire into mneme** - Connect KnowledgeStore to mneme-engine, fact round-trip, HNSW vector search, spawn_blocking wrappers (completed 2026-03-01)
- [x] **Phase 4: Hybrid Retrieval** - BM25 + HNSW + graph join in single Datalog query, RRF fusion, HNSW connectivity validation (completed 2026-03-01)
- [x] **Phase 5: Error + Idiom Migration** - miette to snafu, log to tracing, lazy_static to LazyLock, systematic unwrap audit (completed 2026-03-02)
- [x] **Phase 6: Performance** - Query timeout via cancellation token, ndarray fused distance computation (completed 2026-03-02)
- [x] **Phase 7: Integrate Hybrid Retrieval** - Rebase feat/mneme-engine-p4 onto main, resolve conflicts with Phase 6, verify tests (gap closure) (completed 2026-03-02)
- [x] **Phase 8: Integrate Idiom Migration** - Rebase feat/mneme-engine-p5 onto integrated branch, resolve conflicts, update tracking docs (gap closure) (completed 2026-03-02)

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
- [x] 02-02: SAFETY comment audit across both crates (Wave 1)

### Phase 3: Wire into mneme
**Goal**: KnowledgeStore uses mneme-engine for storage -- facts round-trip through insert+query, HNSW vector search returns nearest neighbors, graph algorithms run safely from async context
**Depends on**: Phase 2
**Requirements**: INTG-01, INTG-02, INTG-03, INTG-04, INTG-05, TEST-02, TEST-03
**Success Criteria** (what must be TRUE):
  1. Integration test creates an in-memory DB, inserts a fact with `Db::run`, queries it back, and gets the same data
  2. Integration test inserts vectors, runs HNSW kNN search, and retrieves the correct nearest neighbors
  3. All graph algorithm calls from KnowledgeStore go through `spawn_blocking` -- no rayon+Tokio deadlock possible
  4. Schema version is tracked in the mneme wrapper and queryable
**Plans**: 2 plans, 2 waves (sequential)

Plans:
- [x] 03-01: Wire dependency, feature-gate errors, implement KnowledgeStore (Wave 1) -- completed 2026-03-01
- [x] 03-02: Integration tests — fact round-trip, HNSW vector search, async wrappers (Wave 2) -- completed 2026-03-01

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
- [x] 04-01: BM25 fixed rule, RRF fixed rule, search_hybrid() (integrated via Phase 7)
- [x] 04-02: Integration tests — hybrid retrieval end-to-end (integrated via Phase 7)

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
- [x] 05-01: log->tracing, lazy_static->LazyLock, env_logger->tracing-subscriber (integrated via Phase 8)
- [x] 05-02: miette->snafu migration (61 files), error.rs, unwrap audit (integrated via Phase 8)

### Phase 6: Performance
**Goal**: Query execution is cancellable via timeout and vector distance computation uses fused ndarray operations
**Depends on**: Phase 5
**Requirements**: PERF-02, PERF-03
**Success Criteria** (what must be TRUE):
  1. A long-running Datalog query can be cancelled by a timeout token, returning a typed error instead of running forever
  2. HNSW distance computation uses ndarray fused operations (`Zip::from(...).apply(...)`) instead of element-wise loops
**Plans**: TBD

Plans:
- [x] 06-01: Query timeout via cancellation token, ndarray fused distance computation (integrated via Phase 7)

### Phase 7: Integrate Hybrid Retrieval
**Goal**: Rebase the 6 `feat/mneme-engine-p4` commits onto current main so hybrid retrieval is available on the integration branch -- BM25, RRF, search_hybrid, and all Phase 4 tests land on main
**Depends on**: Phase 6
**Requirements**: RETR-01, RETR-02, RETR-03, RETR-04, TEST-04, TEST-06, PERF-01
**Gap Closure**: Closes gaps from audit — Phase 4 implemented on `feat/mneme-engine-p4` but never merged
**Success Criteria** (what must be TRUE):
  1. All 6 Phase 4 commits rebased onto current main without regressions
  2. `cargo test -p aletheia-mneme-engine` passes (166+ tests including BM25/RRF)
  3. `cargo test --test knowledge_engine` passes (6+ integration tests including hybrid retrieval)
  4. `search_hybrid()`, `HybridQuery`, `ReciprocalRankFusion` available on integration branch
**Plans**: 1 plan, 1 wave (sequential)

Plans:
- [x] 07-01: Cherry-pick Phase 4 code commits, resolve knowledge_store.rs conflict, squash, verify (Wave 1)

### Phase 8: Integrate Idiom Migration
**Goal**: Rebase the 9 `feat/mneme-engine-p5` commits onto the Phase 7 result so all idiom migrations land -- snafu errors, tracing, LazyLock, unwrap audit, and ABSORPTION.md
**Depends on**: Phase 7
**Requirements**: IDIOM-01, IDIOM-02, IDIOM-03, IDIOM-04, IDIOM-05, DOCS-01
**Gap Closure**: Closes gaps from audit — Phase 5 implemented on `feat/mneme-engine-p5` but never merged; P5 also lacks P4 changes
**Success Criteria** (what must be TRUE):
  1. All 9 Phase 5 commits rebased onto Phase 7 result without regressions
  2. Zero `use miette` or `miette::` imports remain in mneme-engine
  3. Zero `log::` or `use log` imports remain in mneme-engine
  4. Zero `lazy_static!` invocations remain in mneme-engine
  5. `cargo test -p aletheia-mneme-engine` passes with all migrations applied
  6. ABSORPTION.md present in crate docs
  7. ROADMAP.md, REQUIREMENTS.md, STATE.md fully reconciled
**Plans**: 1 plan, 1 wave (sequential)

Plans:
- [x] 08-01: Cherry-pick p5 commits, fix rrf.rs, squash, verify (Wave 1)

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Copy + Compile | 3/3 | Complete   | 2026-03-01 |
| 2. Critical Safety | 2/2 | Complete   | 2026-03-01 |
| 3. Wire into mneme | 2/2 | Complete | 2026-03-01 |
| 4. Hybrid Retrieval | 2/2 | Complete   | 2026-03-01 |
| 5. Error + Idiom Migration | 2/2 | Complete   | 2026-03-02 |
| 6. Performance | 1/1 | Complete   | 2026-03-02 |
| 7. Integrate Hybrid Retrieval | 1/1 | Complete   | 2026-03-02 |
| 8. Integrate Idiom Migration | 1/1 | Complete   | 2026-03-02 |

---

# Roadmap: v2 mneme Subsystem

## Overview

Post-absorption work to make the engine production-ready. v1 proved the absorption works — all 53 requirements met, 268 tests pass. v2 connects the engine to agent capability (recall pipeline), hardens query safety, and lifts the performance ceiling (HNSW redesign). Progressive quality improvement runs continuously alongside feature work.

Source: Issues #405, #408, #409, #411 — consolidated into planning docs, issues closed.

## Principles

1. **Product impact before engineering purity.** Recall pipeline > typed query builder > HNSW redesign > async engine.
2. **Bugs before features.** Graph score aggregation and RRF encoding must be fixed before recall pipeline ships.
3. **Tests travel with features.** No separate "testing phase." Each feature brings its failure-mode tests.
4. **Quality work is opportunistic.** Lint cleanup, unwrap triage, and suppression tightening happen when touching those files for other reasons. Never scheduled as standalone work.

## Phases

### Phase 9: Bug Fixes & Correctness
**Goal**: Fix known correctness issues in hybrid search before wiring it to production recall
**Depends on**: v1 complete (Phase 8)
**Requirements**: BUG-01, BUG-02, BUG-03
**Success Criteria**:
  1. Graph score aggregation sums `graph_raw` scores via `sum()` before RRF consumes them — no duplicate entity tuples
  2. RRF uses -1 (not 0) for missing signal ranks — unambiguous encoding
  3. Hybrid search executes correctly with only 2 signals (BM25 + LSH) when graph rules return empty
  4. Regression tests for all three fixes

### Phase 10: Recall Pipeline
**Goal**: Agent turns retrieve knowledge from KnowledgeStore — the gap between "engine works in tests" and "agents know things"
**Depends on**: Phase 9 (correctness)
**Requirements**: RECALL-01, RECALL-02, RECALL-03, RECALL-04
**Success Criteria**:
  1. `impl VectorSearch for KnowledgeStore` exists and passes trait contract tests
  2. Embedding provider is selected and configured (fastembed-rs local or API-based)
  3. `main.rs` passes live `VectorSearch` impl to `RecallStage` (no more `None`)
  4. End-to-end test: insert knowledge, run agent turn, verify recall influences response
  5. Integration test for recall with empty knowledge store (graceful no-op)

### Phase 11: Typed Query Builder
**Goal**: Compile-time validated Datalog queries — no more format!() string interpolation for schema access
**Depends on**: Phase 10 (recall pipeline working = queries are exercised in production)
**Requirements**: QSAFE-01, QSAFE-02, QSAFE-03, QSAFE-04
**Success Criteria**:
  1. All ~10 KnowledgeStore query patterns use typed builder instead of format!() strings
  2. Builder validates field references at compile time — schema change = compile error, not runtime surprise
  3. Builder emits valid Datalog strings internally (verified by comparing output to known-good queries)
  4. No Datalog injection possible through entity IDs or user-supplied values

### Phase 12: HNSW Redesign
**Goal**: In-memory vector search with WAL persistence — remove the KV-hop performance ceiling
**Depends on**: Phase 10 (recall pipeline exercising HNSW in production gives real performance baseline)
**Requirements**: HNSW-01, HNSW-02, HNSW-03, HNSW-04, DEP-01
**Success Criteria**:
  1. New in-memory HNSW implementation passes same correctness tests as KV-backed version
  2. WAL replay test: kill during write, restart, verify index integrity
  3. Benchmark shows measurable improvement at 10K+ vectors vs. KV-backed version
  4. KV-backed version remains as fallback (feature-gated)
  5. graph-builder absorbed into mneme — rayon pin eliminated
  6. Old graph-builder crate deleted from workspace

### Phase 13: KV Store Evaluation
**Goal**: Assess whether RocksDB is still the right choice after HNSW moves to memory
**Depends on**: Phase 12 (HNSW redesign clarifies what the KV store actually needs to do)
**Requirements**: DEP-02, DEP-03
**Success Criteria**:
  1. Evaluation doc comparing redb, fjall, sled against Aletheia's actual access patterns (facts + relations + FTS indices)
  2. Decision: stay with RocksDB, migrate, or defer. Written with rationale.
  3. If migrating: data migration path documented, correctness tests pass with new backend

### Phase 14: Async Engine
**Goal**: Cooperative async Db::run() — engine works with Tokio instead of fighting it
**Depends on**: Phase 12 (async HNSW search benefits from in-memory design)
**Requirements**: ASYNC-01, ASYNC-02, ASYNC-03
**Success Criteria**:
  1. `Db::run()` yields between evaluation steps — doesn't monopolize blocking thread pool
  2. Existing cancellation token check-points become yield points
  3. All existing tests pass with async path
  4. Benchmark: concurrent query throughput improves vs. spawn_blocking baseline
### Phase 15: Knowledge Lifecycle
**Goal**: Agents populate, maintain, and evolve their knowledge graph — extraction from conversations, conflict resolution on write, consolidation as the graph grows
**Depends on**: Phase 10 (recall pipeline — extraction stage lives adjacent to RecallStage), Phase 11 (typed query builder — extraction writes need schema safety)
**Requirements**: KL-01, KL-02, KL-03, KL-04, KL-05, KL-06, KL-07, KL-08, KL-09, KL-10, KL-11, KL-12
**References**: Zep/Graphiti temporal KG ([arxiv.org/abs/2501.13956](https://arxiv.org/abs/2501.13956)), Mem0 ([arxiv.org/abs/2504.19413](https://arxiv.org/abs/2504.19413))
**Success Criteria**:
  1. Every agent turn triggers extraction pipeline — conversation text → structured triples written to knowledge graph
  2. Extraction is schema-aware — triples conform to relation schema, not freeform
  3. On write, semantic search detects contradictions with existing edges — old facts invalidated with `t_invalid`, not deleted
  4. Conflict log tracks what was superseded and why
  5. Retrieval tracking records access timestamps on edges
  6. Decay scoring integrated into RRF fusion — stale facts rank lower
  7. Consolidation job clusters related facts (Louvain community detection), summarizes via LLM, replaces originals
  8. Explicit forgetting: user/agent can mark facts as forgotten with reason
**Suggested internal sequencing**:
  - Step 1: Entity/relationship extraction (KL-01..04) — populates the graph
  - Step 2: Conflict resolution (KL-05..08) — maintains coherence
  - Step 3: Retrieval tracking (KL-09) — provides data for decay
  - Step 4: Decay scoring (KL-11) — improves retrieval quality
  - Step 5: Consolidation (KL-10) — manages growth
  - Step 6: Explicit forgetting (KL-12) — user control


## Ongoing Work (Not Phased)

These happen continuously, not in dedicated phases:

### Internal Quality (QUAL-01..07)
- Remove unused snafu imports when touching those files
- Tighten lint suppressions one at a time during related work
- Fix string-matching timeout detection when touching error handling
- Progressive unwrap conversion prioritized by call-graph reachability
- from_shape_ptr alignment — may become moot after HNSW redesign
- Storage abstraction simplification — after KV evaluation decision
- Miri testing for unsafe sites — after alignment and HNSW work stabilizes

### Integration Tests (ITEST-01..06)
- Each new feature brings its failure-mode tests
- Concurrent access tests built during recall pipeline work
- Recovery tests built during HNSW redesign (WAL replay covers this)
- Edge case tests accumulated incrementally

### Performance & Capabilities (PERF-V2-01..03, CAP-01..02)
- Query metrics: add when observability infrastructure exists (M5, Spec 41)
- RocksDB backup: add if RocksDB stays after Phase 13 evaluation
- BM25 parameter configurability: add if multi-language support planned
- CSV/JSON import: add when productivity features need it
- FTS tokenizer config: add with multi-language support

## Progress

| Phase | Status | Notes |
|-------|--------|-------|
| 9. Bug Fixes & Correctness | Not started | |
| 10. Recall Pipeline | Not started | Highest priority |
| 11. Typed Query Builder | Not started | |
| 12. HNSW Redesign | Not started | |
| 13. KV Store Evaluation | Not started | |
| 14. Async Engine | Not started | |
| 15. Knowledge Lifecycle | Not started | Extraction + conflict resolution after Phase 10-11; consolidation at scale |

---
*v2 roadmap created: 2026-03-03*
*Source: Issues #405, #408, #409, #411*
