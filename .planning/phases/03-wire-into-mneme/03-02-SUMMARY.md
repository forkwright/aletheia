---
phase: 03-wire-into-mneme
plan: 02
subsystem: integration-tests + mneme
tags: [integration, cozo, knowledge-store, hnsw, testing, feature-flags]
dependency_graph:
  requires: [03-01]
  provides: [TEST-02, TEST-03, INTG-04, INTG-05]
  affects: [crates/integration-tests, crates/mneme]
tech_stack:
  added: [tokio (integration-tests dev-dep), engine-tests feature, sqlite-tests feature]
  patterns: [feature-gated integration tests, per-run in-memory CozoDB stores, spawn_blocking async test]
key_files:
  created:
    - crates/integration-tests/tests/knowledge_engine.rs
  modified:
    - crates/integration-tests/Cargo.toml
    - crates/integration-tests/tests/mneme_session.rs
    - crates/integration-tests/tests/hermeneus_from_mneme.rs
    - crates/integration-tests/tests/nous_session_state.rs
    - crates/mneme/src/knowledge_store.rs
decisions:
  - "schema_version not _schema_version — CozoDB stores underscore-prefixed relations in temp_store_tx (per-run) not persistent store_tx; renaming to schema_version makes it survive across run() calls"
  - "engine-tests / sqlite-tests as Cargo features — cleanest way to separate incompatible sqlite3/cozo link units; default stays sqlite-tests for backward compat"
  - "MockEmbeddingProvider dim=16 for HNSW test — smaller dimension reduces test time while preserving nearest-neighbor ordering property"
metrics:
  duration_minutes: 5
  tasks_completed: 1
  files_modified: 5
  completed_date: "2026-03-01"
requirements:
  - TEST-02
  - TEST-03
---

# Phase 3 Plan 2: Integration tests for KnowledgeStore (TEST-02, TEST-03)

Six integration tests proving KnowledgeStore works end-to-end: fact round-trip through CozoDB Datalog, HNSW kNN vector search with nearest-neighbor verification, schema version tracking, multi-fact ordering, async spawn_blocking wrappers, and raw query escape hatch.

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Add mneme-engine to integration-tests and create knowledge_engine.rs | 8724dbb | Cargo.toml, knowledge_engine.rs, mneme_session.rs, hermeneus_from_mneme.rs, nous_session_state.rs, knowledge_store.rs |

## What Was Built

**Cargo.toml restructuring:**
- Changed `aletheia-mneme` to `default-features = false` — prevents sqlite3 link conflict
- Added `engine-tests = ["aletheia-mneme/mneme-engine"]` and `sqlite-tests = ["aletheia-mneme/sqlite"]` features
- Added `tokio = { version = "1", features = ["rt-multi-thread", "macros"] }` for async tests
- Default feature remains `sqlite-tests` — all existing CI passes without flag changes

**Feature gating of sqlite-dependent tests:**
- `mneme_session.rs`: added `#![cfg(feature = "sqlite-tests")]` — uses `store::SessionStore`
- `hermeneus_from_mneme.rs`: added `#![cfg(feature = "sqlite-tests")]` — uses `store::SessionStore`
- `nous_session_state.rs`: added `#![cfg(feature = "sqlite-tests")]` — uses `store::SessionStore`

**knowledge_engine.rs — 6 integration tests:**

1. `fact_round_trip` (TEST-02): inserts a Fact with all 10 fields, queries via FULL_CURRENT_FACTS, verifies id/content/confidence/tier fields match exactly
2. `hnsw_vector_search` (TEST-03): inserts 6 embeddings via MockEmbeddingProvider (dim=16), queries with "apple" vector, verifies apple is nearest neighbor with cosine distance < 0.01
3. `schema_version_queryable` (INTG-05): opens store, queries schema_version relation, asserts version == 1
4. `multiple_facts_ordered_by_confidence`: inserts 3 facts at confidences 0.5/0.9/0.7, verifies descending order (0.9, 0.7, 0.5)
5. `async_spawn_blocking_wrapper` (INTG-04): uses `#[tokio::test]`, calls `insert_fact_async` and `query_facts_async`, verifies result from async context
6. `raw_query_escape_hatch`: calls `run_query("::relations", ...)`, verifies >= 5 relations returned

**Bug fix in knowledge_store.rs:**
- Renamed `_schema_version` relation to `schema_version` throughout `init_schema()` and `schema_version()` methods

## Verification Results

```
cargo test -p aletheia-integration-tests --no-default-features --features engine-tests  -> PASS (6/6 tests)
cargo test -p aletheia-integration-tests --features sqlite-tests                         -> PASS (all sqlite tests)
cargo check --workspace                                                                  -> PASS
cargo clippy -p aletheia-integration-tests --no-default-features --features engine-tests -> PASS (0 warnings)
```

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `_schema_version` uses CozoDB temporary store (not persistent)**
- **Found during:** Task 1 — first test run (all 6 tests failed with "Cannot find requested stored relation '_schema_version'")
- **Issue:** CozoDB stores relations with underscore prefix (`_`) in `temp_store_tx` (per-transaction temporary store) rather than `store_tx` (persistent store). Each `db.run()` call creates a fresh transaction, so the `_schema_version` relation created in one `run()` was invisible to subsequent `run()` calls — causing "Cannot find requested stored relation" on the insert, and on every later query.
- **Fix:** Renamed `_schema_version` to `schema_version` (no underscore) in `init_schema()` DDL, the insert query, and the `schema_version()` read method. The non-underscore name uses `store_tx` which persists across `run()` calls on the same `Db` instance.
- **Files modified:** crates/mneme/src/knowledge_store.rs
- **Commit:** 8724dbb

## Self-Check: PASSED

- crates/integration-tests/tests/knowledge_engine.rs: FOUND
- crates/integration-tests/Cargo.toml: FOUND (engine-tests/sqlite-tests features)
- crates/integration-tests/tests/mneme_session.rs: FOUND (#![cfg(feature = "sqlite-tests")] line 2)
- crates/integration-tests/tests/hermeneus_from_mneme.rs: FOUND (#![cfg(feature = "sqlite-tests")] line 2)
- crates/integration-tests/tests/nous_session_state.rs: FOUND (#![cfg(feature = "sqlite-tests")] line 2)
- crates/mneme/src/knowledge_store.rs: FOUND (schema_version not _schema_version)
- Commit 8724dbb (Task 1): FOUND
