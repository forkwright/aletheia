# Requirements: v1.0 CozoDB Absorption

**Defined:** 2026-03-01
**Core Value:** Every milestone produces a PR that meets the absolute quality bar

## v1 Requirements

Requirements for the full CozoDB absorption into mneme-engine. Covers compile, strip, safety, integration, hybrid retrieval, error migration, and performance.

### Crate Structure

- [x] **CRATE-01**: mneme-engine exists as a workspace crate at `crates/mneme-engine/` containing absorbed CozoDB source
- [x] **CRATE-02**: graph-builder exists as a separate workspace crate at `crates/graph-builder/` isolating 42 unsafe sites and the rayon pin
- [x] **CRATE-03**: `cargo check --workspace` succeeds with both new crates
- [x] **CRATE-04**: CozoDB module structure is preserved verbatim (data/, parse/, query/, runtime/, storage/, fixed_rule/, fts/)
- [x] **CRATE-05**: All internal modules are `pub(crate)` with public API exposed via lib.rs re-exports only

### Stripping

- [x] **STRIP-01**: Storage backends rocks.rs (legacy), sqlite.rs, sled.rs, tikv.rs are removed
- [x] **STRIP-02**: Chinese tokenizer (fts/cangjie/) and jieba-rs dependency are removed
- [x] **STRIP-03**: FFI/binding code removed — all `*_str` methods, `new_with_str`, `#[no_mangle]`, `extern "C"`
- [x] **STRIP-04**: HTTP server code and minreq dependency removed
- [x] **STRIP-05**: CSV/JSON reader utilities removed (not needed for engine)
- [x] **STRIP-06**: DbInstance FFI dispatcher enum removed — `Db<MemStorage>` and `Db<RocksDbStorage>` exposed directly
- [x] **STRIP-07**: graph-builder compat.rs polyfills removed (replaced with stdlib equivalents)
- [x] **STRIP-08**: Stopword files trimmed to English-only (remove 20K+ lines of CJK/multilingual stopwords)

### Public API

- [x] **API-01**: `Db::open_rocksdb(path)` and `Db::open_mem()` constructors available
- [x] **API-02**: `Db::run(script, params, mutability)` executes Datalog queries
- [x] **API-03**: `Db::export_relations` and `Db::import_relations` available for backup/restore
- [x] **API-04**: `Db::register_fixed_rule` available for custom graph algorithms
- [x] **API-05**: `ValidityTs` exposed as public type for bi-temporal queries
- [x] **API-06**: `Db::register_callback` exposed for change notifications
- [x] **API-07**: `Db::multi_transaction` exposed for atomic multi-relation writes
- [x] **API-08**: `DataValue`, `NamedRows`, `ScriptMutability` exposed as public types

### Safety

- [x] **SAFE-01**: minhash_lsh.rs:310 unsound `&[u8]` to `&[u32]` cast replaced with bytemuck
- [x] **SAFE-02**: All remaining unsafe sites documented with SAFETY comments
- [x] **SAFE-03**: `static_assertions::assert_impl_all!` verifies Send+Sync on key public types
- [x] **SAFE-04**: env_logger moved to dev-dependencies only (resolves tracing conflict)
- [x] **SAFE-05**: newrocks.rs unsafe Sync impl documented with safety justification

### Integration

- [x] **INTG-01**: mneme-engine is a dependency of aletheia-mneme
- [x] **INTG-02**: KnowledgeStore uses mneme-engine for fact storage (insert + query round-trip works)
- [x] **INTG-03**: KnowledgeStore uses mneme-engine for HNSW vector search (insert + knn works)
- [x] **INTG-04**: Graph algorithm calls wrapped in `spawn_blocking` to prevent rayon+Tokio deadlock
- [x] **INTG-05**: Schema version tracking implemented in mneme wrapper
- [x] **INTG-06**: `vendor/cozo/` and `vendor/graph_builder/` deletable after absorption (all code in crates/)

### Hybrid Retrieval

- [x] **RETR-01**: BM25 full-text search executes as a Datalog query
- [x] **RETR-02**: HNSW vector similarity search executes as a Datalog query
- [x] **RETR-03**: Combined BM25 + HNSW + graph join retrieval works in a single Datalog query
- [x] **RETR-04**: RRF (Reciprocal Rank Fusion) merging happens within the engine, not app-layer

### Error and Idiom Migration

- [x] **IDIOM-01**: miette error types migrated to snafu pattern
- [x] **IDIOM-02**: log macros replaced with tracing macros
- [x] **IDIOM-03**: lazy_static replaced with LazyLock
- [x] **IDIOM-04**: Systematic unwrap audit — unwraps in paths reachable from public API converted to typed errors
- [x] **IDIOM-05**: env_logger fully removed (replaced by tracing subscriber in test harness)

### Performance

- [x] **PERF-01**: HNSW connectivity verification test exists to detect recall degradation
- [x] **PERF-02**: Query timeout via cancellation token in eval loop
- [x] **PERF-03**: ndarray fused operations for distance computation where applicable

### Testing

- [x] **TEST-01**: CozoDB's own test suite (`runtime/tests.rs`) passes under mneme-engine
- [x] **TEST-02**: Integration test: create DB, insert fact, query fact, verify round-trip
- [x] **TEST-03**: Integration test: create DB, insert vectors, HNSW knn search, verify results
- [x] **TEST-04**: Integration test: hybrid retrieval (BM25 + HNSW + graph) end-to-end
- [x] **TEST-05**: `cargo clippy --workspace` clean (crate-level `#[expect]` for inherited CozoDB warnings)
- [x] **TEST-06**: HNSW soft-deletion connectivity test (verify <5% degradation after N cycles)

### Documentation

- [x] **DOCS-01**: ABSORPTION.md documents lines removed (before/after), unsafe sites carried, unwraps carried, remaining cleanup
- [x] **DOCS-02**: UPSTREAM-REVIEW.md documents reviewed PRs/issues/branches with disposition
- [x] **DOCS-03**: VENDORED.md documents source origin, version, copyright headers preserved


## v2 Requirements

Post-v1.0 work. Organized by priority tier based on product impact vs. engineering quality.
Source: Issues #405, #408, #409 — consolidated here as canon.

### Recall Pipeline (Highest Priority — Lights Up Knowledge Retrieval)

- **RECALL-01**: `impl VectorSearch for KnowledgeStore` bridging the trait to mneme-engine's HNSW
- **RECALL-02**: Embedding provider selection and integration — which model/service generates vectors for agent turns
- **RECALL-03**: Wire `vector_search` + `embedding_provider` in `main.rs` so `RecallStage` gets a live `VectorSearch` impl (currently `None`)
- **RECALL-04**: End-to-end test: agent turn triggers recall, recall hits KnowledgeStore, results influence response

### Query Safety

- **QSAFE-01**: Typed query builder for KnowledgeStore's ~10 Datalog patterns — compile-time schema validation, no format!() string injection
- **QSAFE-02**: Builder emits Datalog strings internally (safety layer, not engine replacement)
- **QSAFE-03**: IDE-navigable field references (typed `Field::Entity` instead of string `"entity"`)
- **QSAFE-04**: Unit tests validating generated Datalog against known-good queries

### HNSW Redesign (Highest Long-Term Impact)

- **HNSW-01**: In-memory HNSW with WAL persistence — vectors live in memory, WAL provides durability, search is pointer chasing not KV hops
- **HNSW-02**: New implementation alongside existing KV-backed version (correctness reference and fallback)
- **HNSW-03**: Benchmark suite: 1K, 10K, 100K vectors — before/after comparison on realistic workloads
- **HNSW-04**: WAL replay correctness test — kill process during write, restart, verify data integrity and index consistency

### Dependency Health

- **DEP-01**: Resolve rayon `=1.10.0` pin in graph-builder — fix for rayon 1.11+, replace with stdlib/tokio parallelism, or absorb graph-builder into mneme during HNSW redesign
- **DEP-02**: Evaluate Rust-native KV alternatives to RocksDB (redb, fjall, sled) — correctness first, then build simplicity, then performance
- **DEP-03**: KV evaluation deferred until after HNSW redesign clarifies actual KV store responsibilities (facts + relations + FTS only if vectors move to memory)

### Async Engine

- **ASYNC-01**: Async `Db::run()` path — yield between Datalog evaluation steps or at I/O boundaries
- **ASYNC-02**: Leverage existing cancellation token check-points in eval loop as yield points
- **ASYNC-03**: Benefits from HNSW redesign first (async HNSW search)

### Integration Testing

- **ITEST-01**: Failure mode coverage — RocksDB corruption, query timeout mid-write, HNSW index inconsistency
- **ITEST-02**: Concurrent access — multiple `spawn_blocking` calls hitting engine simultaneously, lock contention under load
- **ITEST-03**: Recovery — kill process during write, restart, verify data integrity
- **ITEST-04**: Schema migration — upgrade schema version, verify old data readable
- **ITEST-05**: Edge cases — empty knowledge graph queries, maximum vector dimensions, Unicode in entity IDs, Datalog injection attempts
- **ITEST-06**: Build incrementally with each feature, not as a separate phase

### Internal Quality (Progressive, Ongoing)

- **QUAL-01**: Remove 72 unused snafu imports (Phase 8 residue) — mechanical, no logic changes
- **QUAL-02**: Tighten Cargo.toml lint suppressions — remove one at a time, fix or `#[expect]` each individually
- **QUAL-03**: Replace string-matching timeout detection (`"killed before completion"`) with error type/variant matching
- **QUAL-04**: Progressive unwrap triage — prioritize by call-graph depth from public API, not blanket conversion. Many are genuinely infallible.
- **QUAL-05**: `from_shape_ptr` alignment fix (4 sites in HNSW) — store as native f32/f64 bytes with guaranteed alignment, or use `bytemuck::try_cast_slice`. May become moot if HNSW redesign replaces KV-backed path.
- **QUAL-06**: Storage abstraction simplification — `StoreTx` trait designed for 6 backends, Aletheia uses 2. Simplify to actual access patterns.
- **QUAL-07**: Comprehensive unsafe audit with miri testing where applicable (SAFE-V2-01 from v1)

### Performance Monitoring

- **PERF-V2-01**: Query statistics and metrics exposition
- **PERF-V2-02**: RocksDB checkpoint-based physical backup
- **PERF-V2-03**: BM25 parameter configurability (k1, b) — currently hardcoded at k1=1.2, b=0.75. Flag for future if multi-language support added.

### Capability Additions

- **CAP-01**: CSV/JSON import readers for KnowledgeStore — re-implement as KnowledgeStore method (original CozoDB readers stripped in v1, were tightly coupled to DbInstance FFI layer)
- **CAP-02**: FTS tokenizer configurability — move 'Simple' tokenizer from hardcoded to KnowledgeConfig if multi-language support planned

### Knowledge Lifecycle (Source: #411)

Informed by Zep/Graphiti ([arxiv.org/abs/2501.13956](https://arxiv.org/abs/2501.13956)) and Mem0 ([arxiv.org/abs/2504.19413](https://arxiv.org/abs/2504.19413)).

#### Entity/Relationship Extraction

- **KL-01**: LLM-based extraction pipeline stage in nous (adjacent to RecallStage) — conversation text → structured (entity, relationship, entity) triples
- **KL-02**: Schema-aware extraction — triples conform to knowledge graph's relation schema, written as `:put` operations
- **KL-03**: Incremental processing — new turns only, not full conversation history
- **KL-04**: Configurable extraction scope — which conversations produce knowledge (not every chat is worth remembering)

#### Conflict Resolution

- **KL-05**: Semantic contradiction detection on write — search existing edges in entity neighborhood for conflicts with new fact
- **KL-06**: Invalidation, not deletion — old facts get `t_invalid` set, remain queryable for temporal reasoning
- **KL-07**: Conflict severity classification — hard conflict (supersede), soft conflict (coexist), temporal supersession (boundary)
- **KL-08**: Conflict log — track what was superseded and why (auditable knowledge evolution)

#### Memory Consolidation and Forgetting

- **KL-09**: Retrieval tracking — record when facts are retrieved (access timestamp on edges) for decay/consolidation scoring
- **KL-10**: Consolidation job — periodic process using community detection (Louvain) to cluster related facts, LLM summarizes cluster, writes summary fact, invalidates originals
- **KL-11**: Decay scoring in retrieval — RRF fusion incorporates recency of last access + frequency of access + temporal validity as additional signal
- **KL-12**: Explicit forgetting — user or agent marks facts as forgotten, invalidated with reason, excluded from future retrieval

### Known Bugs (From Code Review)

- **BUG-01**: Graph score aggregation missing in hybrid search — multiple relations for same entity produce separate tuples instead of summed scores before RRF ranking. Must fix before production use.
- **BUG-02**: RRF rank encoding ambiguity — using 0 for "not found" creates confusion with impossible rank 0. Recommend -1 for missing signals.
- **BUG-03**: Empty `seed_entities` behavior — verify RRF still executes correctly with only 2 signals (BM25 + LSH) when graph rules return empty

## v2 Sequencing

```
Tier 1 (Now):       Recall Pipeline (RECALL-01..04)     — makes the engine matter
                    Bug fixes (BUG-01..03)               — correctness before features
Tier 2 (Soon):      Query Safety (QSAFE-01..04)          — before patterns multiply
                    Knowledge Extraction (KL-01..04)     — populates the graph
                    Conflict Resolution (KL-05..08)      — maintains graph coherence
Tier 3 (With HNSW): Absorb graph-builder (DEP-01)        — kill rayon pin during redesign
                    HNSW Redesign (HNSW-01..04)          — performance ceiling lift
Tier 4 (Later):     KV Evaluation (DEP-02..03)           — after HNSW clarifies needs
                    Async Engine (ASYNC-01..03)          — unlocks scale
                    Consolidation (KL-09..12)            — manages graph growth at scale
Ongoing:            Internal Quality (QUAL-01..07)       — opportunistic, not scheduled
                    Integration Tests (ITEST-01..06)     — build with each feature
Demand-driven:      Capabilities (CAP-01..02)            — when features need them
                    Perf Monitoring (PERF-V2-01..03)     — when operating at scale
```

## Out of Scope

| Feature | Reason |
|---------|--------|
| SQLite backup methods | Backend stripped — rusqlite used separately in other crates |
| String-based FFI API | Rust-native only — no C/Java/Python/WASM consumers |
| Generic `Db<S>` type exposure | Concrete types via facade — KnowledgeStore wraps anyway |
| Embedding generation | Belongs in mneme::embed, not the storage engine |
| CozoDB upstream tracking | Upstream inactive since Dec 2024, no divergence risk |
| Refactoring CozoDB internal patterns | Compile + absorb + integrate. Deep refactoring is separate future work beyond error migration. |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| CRATE-01 | Phase 1 | Complete |
| CRATE-02 | Phase 1 | Complete |
| CRATE-03 | Phase 1 | Complete |
| CRATE-04 | Phase 1 | Complete |
| CRATE-05 | Phase 1 | Complete |
| STRIP-01 | Phase 1 | Complete |
| STRIP-02 | Phase 1 | Complete |
| STRIP-03 | Phase 1 | Complete |
| STRIP-04 | Phase 1 | Complete |
| STRIP-05 | Phase 1 | Complete |
| STRIP-06 | Phase 1 | Complete |
| STRIP-07 | Phase 1 | Complete |
| STRIP-08 | Phase 1 | Complete |
| API-01 | Phase 1 | Complete |
| API-02 | Phase 1 | Complete |
| API-03 | Phase 1 | Complete |
| API-04 | Phase 1 | Complete |
| API-05 | Phase 1 | Complete |
| API-06 | Phase 1 | Complete |
| API-07 | Phase 1 | Complete |
| API-08 | Phase 1 | Complete |
| SAFE-01 | Phase 2 | Complete |
| SAFE-02 | Phase 2 | Complete |
| SAFE-03 | Phase 2 | Complete |
| SAFE-04 | Phase 1 | Complete |
| SAFE-05 | Phase 2 | Complete |
| INTG-01 | Phase 3 (03-01) | Complete |
| INTG-02 | Phase 3 (03-02) | Complete |
| INTG-03 | Phase 3 (03-02) | Complete |
| INTG-04 | Phase 3 (03-01) | Complete |
| INTG-05 | Phase 3 (03-01) | Complete |
| INTG-06 | Phase 1 | Complete |
| RETR-01 | Phase 7 (gap closure) | Complete |
| RETR-02 | Phase 7 (gap closure) | Complete |
| RETR-03 | Phase 7 (gap closure) | Complete |
| RETR-04 | Phase 7 (gap closure) | Complete |
| IDIOM-01 | Phase 8 (gap closure) | Complete |
| IDIOM-02 | Phase 8 (gap closure) | Complete |
| IDIOM-03 | Phase 8 (gap closure) | Complete |
| IDIOM-04 | Phase 8 (gap closure) | Complete |
| IDIOM-05 | Phase 8 (gap closure) | Complete |
| PERF-01 | Phase 7 (gap closure) | Complete |
| PERF-02 | Phase 6 | Complete |
| PERF-03 | Phase 6 | Complete |
| TEST-01 | Phase 1 | Complete |
| TEST-02 | Phase 3 | Complete |
| TEST-03 | Phase 3 | Complete |
| TEST-04 | Phase 7 (gap closure) | Complete |
| TEST-05 | Phase 1 | Complete |
| TEST-06 | Phase 7 (gap closure) | Complete |
| DOCS-01 | Phase 8 (gap closure) | Complete |
| DOCS-02 | Phase 1 | Complete |
| DOCS-03 | Phase 1 | Complete |

**Coverage:**
- v1 requirements: 53 total
- Satisfied on main: 53
- Pending integration: 0
- Unmapped: 0

**Per-phase breakdown:**
- Phase 1 (Copy + Compile): 27 requirements
- Phase 2 (Critical Safety): 4 requirements
- Phase 3 (Wire into mneme): 7 requirements
- Phase 4 (Hybrid Retrieval): 0 requirements (reassigned to Phase 7)
- Phase 5 (Error + Idiom Migration): 0 requirements (reassigned to Phase 8)
- Phase 6 (Performance): 2 requirements
- Phase 7 (Integrate Hybrid Retrieval): 7 requirements (gap closure)
- Phase 8 (Integrate Idiom Migration): 6 requirements (gap closure)

---
*Requirements defined: 2026-03-01*
*Last updated: 2026-03-03 -- v2 requirements integrated from issues #405, #408, #409, #411*
