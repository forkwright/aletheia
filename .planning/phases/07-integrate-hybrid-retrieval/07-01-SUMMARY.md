---
phase: 07-integrate-hybrid-retrieval
plan: 01
subsystem: database
tags: [cozo, datalog, bm25, hnsw, rrf, hybrid-retrieval, fts, mneme-engine]

# Dependency graph
requires:
  - phase: 04-hybrid-retrieval
    provides: "Phase 4 feature branch with BM25, RRF, search_hybrid — never merged to main"
  - phase: 06-performance
    provides: "run_query_with_timeout, ndarray Zip fusion, query timeout error type"

provides:
  - "BM25 full-text search via FtsScoreKind::Bm25 in mneme-engine"
  - "ReciprocalRankFusion FixedRule registered in DEFAULT_FIXED_RULES, callable from Datalog"
  - "search_hybrid() on KnowledgeStore combining BM25 + HNSW + graph signals via RRF"
  - "HybridQuery and HybridResult public types on mneme crate"
  - "fts_ddl() creating facts:content_fts index in init_schema()"
  - "run_mut_query() escape hatch for mutable Datalog operations"
  - "hybrid_retrieval_end_to_end and hnsw_connectivity integration tests"

affects: [08-mneme-api, nous-recall-pipeline, aletheia-binary]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Cherry-pick + manual conflict resolution for feature branch integration across diverged history"
    - "Additive conflict resolution: both Phase 6 timeout and Phase 4 hybrid methods kept"
    - "Squash to single commit for clean history when cherry-picks have internal dependencies"

key-files:
  created:
    - crates/mneme-engine/src/fixed_rule/utilities/rrf.rs
  modified:
    - crates/mneme-engine/src/fts/indexing.rs
    - crates/mneme-engine/src/data/program.rs
    - crates/mneme-engine/src/fixed_rule/mod.rs
    - crates/mneme-engine/src/fixed_rule/utilities/mod.rs
    - crates/mneme/src/knowledge_store.rs
    - crates/integration-tests/tests/knowledge_engine.rs

key-decisions:
  - "Single squash commit chosen over 2-commit split — all cherry-picks were staged together after reset --soft; cleaner than forcing artificial split"
  - "Conflict resolution: Phase 6 timeout block retained first, Phase 4 hybrid methods appended after — both sets fully preserved, additive not substitutive"
  - "init_schema() fts_ddl() call already present in cherry-picked file — no manual wiring needed"

patterns-established:
  - "Conflict resolution order: Phase 6 additions (run_query_with_timeout) precede Phase 4 additions (search_hybrid) in knowledge_store.rs"
  - "Hybrid query builds dynamic Datalog with inline seed expansion to avoid is_in() dependency"

requirements-completed: [RETR-01, RETR-02, RETR-03, RETR-04, TEST-04, TEST-06, PERF-01]

# Metrics
duration: 7min
completed: 2026-03-02
---

# Phase 7 Plan 01: Integrate Hybrid Retrieval Summary

**BM25 scoring, ReciprocalRankFusion fixed rule, and search_hybrid() landed on main by cherry-picking 5 commits from feat/mneme-engine-p4 with one manual conflict resolution**

## Performance

- **Duration:** 7 min
- **Started:** 2026-03-02T17:09:02Z
- **Completed:** 2026-03-02T17:16:02Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments

- All 5 Phase 4 cherry-picks applied (2 clean, 1 conflict-resolved, 1 auto-applied on resolved, 1 clean)
- knowledge_store.rs conflict resolved additively: `run_query_with_timeout()` (Phase 6) and `search_hybrid()` (Phase 4) coexist
- 173 mneme-engine tests pass including 7 BM25 unit tests
- 66 mneme tests pass including hybrid query builder tests and timeout tests
- Integration tests pass: `hybrid_retrieval_end_to_end` and `hnsw_connectivity_after_delete_reinsert_cycles`
- Clippy clean across workspace

## Task Commits

Each task was committed atomically:

1. **Task 1: Cherry-pick clean commits (BM25 + RRF engine code)** — via cherry-picks `9022d7fd` + `7750257a` (then squashed)
2. **Task 2: Resolve conflict, land hybrid API + tests** — `f33f05e7` (squashed all cherry-picks into single commit on main)

**Final commit on main:** `f33f05e7`

## Files Created/Modified

- `crates/mneme-engine/src/fixed_rule/utilities/rrf.rs` — ReciprocalRankFusion FixedRule implementation (116 lines, arity 5, RRF_K=60.0)
- `crates/mneme-engine/src/fts/indexing.rs` — bm25_compute_score(), FtsScoreKind::Bm25, avg_dl_cache, doc_len restoration
- `crates/mneme-engine/src/data/program.rs` — FtsScoreKind::Bm25 variant + 'bm25' parser arm
- `crates/mneme-engine/src/fixed_rule/mod.rs` — ReciprocalRankFusion registered in DEFAULT_FIXED_RULES
- `crates/mneme-engine/src/fixed_rule/utilities/mod.rs` — pub(crate) mod rrf + re-export
- `crates/mneme/src/knowledge_store.rs` — HybridQuery, HybridResult, search_hybrid(), search_hybrid_async(), run_mut_query(), fts_ddl(), build_hybrid_query(), rows_to_hybrid_results()
- `crates/integration-tests/tests/knowledge_engine.rs` — hybrid_retrieval_end_to_end, hnsw_connectivity_after_delete_reinsert_cycles

## Decisions Made

- Squashed all cherry-picks into 1 commit (Option A from plan) — `git reset --soft main` staged all changes together before the split could be made; single commit is cleaner than amending
- Conflict resolution strategy: additive — both Phase 6 `run_query_with_timeout()` and Phase 4 `search_hybrid()` group kept; Phase 6 block first as per locked decision
- `fts_ddl()` call in `init_schema()` verified intact post-resolution (line 325)

## Deviations from Plan

None - plan executed exactly as written. The conflict in knowledge_store.rs was expected and resolved per the documented resolution rules. Cherry-picks 2, 4, and 5 applied cleanly as predicted.

## Issues Encountered

None. All 5 cherry-picks applied as expected. The single conflict on knowledge_store.rs was fully documented with resolution rules in the plan context.

## Next Phase Readiness

- All hybrid retrieval APIs available on main for consumption by nous recall pipeline
- `search_hybrid()` callable via `Arc<KnowledgeStore>` from async contexts via `search_hybrid_async()`
- Phase 6 query timeout capability preserved alongside new hybrid APIs
- Ready for Phase 7 Plan 02 if one exists, or Phase 8

---
*Phase: 07-integrate-hybrid-retrieval*
*Completed: 2026-03-02*
