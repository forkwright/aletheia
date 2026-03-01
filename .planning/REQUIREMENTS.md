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

- [ ] **INTG-01**: mneme-engine is a dependency of aletheia-mneme
- [ ] **INTG-02**: KnowledgeStore uses mneme-engine for fact storage (insert + query round-trip works)
- [ ] **INTG-03**: KnowledgeStore uses mneme-engine for HNSW vector search (insert + knn works)
- [ ] **INTG-04**: Graph algorithm calls wrapped in `spawn_blocking` to prevent rayon+Tokio deadlock
- [ ] **INTG-05**: Schema version tracking implemented in mneme wrapper
- [x] **INTG-06**: `vendor/cozo/` and `vendor/graph_builder/` deletable after absorption (all code in crates/)

### Hybrid Retrieval

- [ ] **RETR-01**: BM25 full-text search executes as a Datalog query
- [ ] **RETR-02**: HNSW vector similarity search executes as a Datalog query
- [ ] **RETR-03**: Combined BM25 + HNSW + graph join retrieval works in a single Datalog query
- [ ] **RETR-04**: RRF (Reciprocal Rank Fusion) merging happens within the engine, not app-layer

### Error and Idiom Migration

- [ ] **IDIOM-01**: miette error types migrated to snafu pattern
- [ ] **IDIOM-02**: log macros replaced with tracing macros
- [ ] **IDIOM-03**: lazy_static replaced with LazyLock
- [ ] **IDIOM-04**: Systematic unwrap audit — unwraps in paths reachable from public API converted to typed errors
- [ ] **IDIOM-05**: env_logger fully removed (replaced by tracing subscriber in test harness)

### Performance

- [ ] **PERF-01**: HNSW connectivity verification test exists to detect recall degradation
- [ ] **PERF-02**: Query timeout via cancellation token in eval loop
- [ ] **PERF-03**: ndarray fused operations for distance computation where applicable

### Testing

- [x] **TEST-01**: CozoDB's own test suite (`runtime/tests.rs`) passes under mneme-engine
- [ ] **TEST-02**: Integration test: create DB, insert fact, query fact, verify round-trip
- [ ] **TEST-03**: Integration test: create DB, insert vectors, HNSW knn search, verify results
- [ ] **TEST-04**: Integration test: hybrid retrieval (BM25 + HNSW + graph) end-to-end
- [x] **TEST-05**: `cargo clippy --workspace` clean (crate-level `#[expect]` for inherited CozoDB warnings)
- [ ] **TEST-06**: HNSW soft-deletion connectivity test (verify <5% degradation after N cycles)

### Documentation

- [ ] **DOCS-01**: ABSORPTION.md documents lines removed (before/after), unsafe sites carried, unwraps carried, remaining cleanup
- [x] **DOCS-02**: UPSTREAM-REVIEW.md documents reviewed PRs/issues/branches with disposition
- [x] **DOCS-03**: VENDORED.md documents source origin, version, copyright headers preserved

## v2 Requirements

Deferred beyond this milestone.

- **PERF-V2-01**: In-memory HNSW with WAL persistence (full redesign of KV-backed HNSW)
- **PERF-V2-02**: Async `Db::run()` via `spawn_blocking` wrapper
- **PERF-V2-03**: RocksDB checkpoint-based physical backup
- **PERF-V2-04**: Query statistics and metrics exposition
- **SAFE-V2-01**: Comprehensive unsafe audit with miri testing where applicable

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
| INTG-01 | Phase 3 | Pending |
| INTG-02 | Phase 3 | Pending |
| INTG-03 | Phase 3 | Pending |
| INTG-04 | Phase 3 | Pending |
| INTG-05 | Phase 3 | Pending |
| INTG-06 | Phase 1 | Complete |
| RETR-01 | Phase 4 | Pending |
| RETR-02 | Phase 4 | Pending |
| RETR-03 | Phase 4 | Pending |
| RETR-04 | Phase 4 | Pending |
| IDIOM-01 | Phase 5 | Pending |
| IDIOM-02 | Phase 5 | Pending |
| IDIOM-03 | Phase 5 | Pending |
| IDIOM-04 | Phase 5 | Pending |
| IDIOM-05 | Phase 5 | Pending |
| PERF-01 | Phase 4 | Pending |
| PERF-02 | Phase 6 | Pending |
| PERF-03 | Phase 6 | Pending |
| TEST-01 | Phase 1 | Complete |
| TEST-02 | Phase 3 | Pending |
| TEST-03 | Phase 3 | Pending |
| TEST-04 | Phase 4 | Pending |
| TEST-05 | Phase 1 | Complete |
| TEST-06 | Phase 4 | Pending |
| DOCS-01 | Phase 5 | Pending |
| DOCS-02 | Phase 1 | Complete |
| DOCS-03 | Phase 1 | Complete |

**Coverage:**
- v1 requirements: 53 total
- Mapped to phases: 53
- Unmapped: 0

**Per-phase breakdown:**
- Phase 1 (Copy + Compile): 27 requirements
- Phase 2 (Critical Safety): 4 requirements
- Phase 3 (Wire into mneme): 7 requirements
- Phase 4 (Hybrid Retrieval): 7 requirements
- Phase 5 (Error + Idiom Migration): 6 requirements
- Phase 6 (Performance): 2 requirements

---
*Requirements defined: 2026-03-01*
*Last updated: 2026-03-01 -- 01-02 complete: CRATE-05, STRIP-01..08, API-01..08 marked complete*
