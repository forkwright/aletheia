---
gsd_state_version: 1.0
milestone: v2
milestone_name: mneme-v2
status: planning
last_updated: "2026-03-03T00:00:00.000Z"
progress:
  total_phases: 7
  completed_phases: 0
  total_plans: 0
  completed_plans: 0
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-01)
See: docs/PROJECT.md for full project roadmap

**Core value:** Every milestone produces a PR that meets the absolute quality bar
**Current focus:** v2 mneme subsystem — recall pipeline, query safety, HNSW redesign

## Current Position

Phase: 9 of 15 (Bug Fixes & Correctness) — NOT STARTED
Status: v1 CozoDB absorption COMPLETE (53/53 requirements, 268 tests). v2 planning docs integrated from issues #405, #408, #409, #411.
Last activity: 2026-03-03 — v2 requirements and roadmap integrated into planning docs. Issues #405, #408, #409, #411 to be closed.

Progress: [░░░░░░░░░░] 0%

## v1 Milestone: CozoDB Absorption — COMPLETE

All 8 phases complete. 53/53 requirements satisfied. 268 tests passing. PR #406 merged.
Full v1 details preserved in ROADMAP.md (top section) and REQUIREMENTS.md (v1 section).

**Performance Metrics (v1):**
- Total plans completed: 15
- Total execution time: ~2.5 hours
- Average duration: ~20 min per plan

## v2 Milestone: mneme Subsystem Improvements

**Scope:** Connect engine to agent capability, harden query safety, lift performance ceiling.

| Phase | Requirements | Status | Notes |
|-------|-------------|--------|-------|
| 9. Bug Fixes & Correctness | BUG-01..03 | Not started | Graph score aggregation, RRF encoding, empty seed_entities |
| 10. Recall Pipeline | RECALL-01..04 | Not started | **Highest priority** — makes the engine matter |
| 11. Typed Query Builder | QSAFE-01..04 | Not started | Compile-time Datalog validation |
| 12. HNSW Redesign | HNSW-01..04, DEP-01 | Not started | In-memory + WAL, absorb graph-builder |
| 13. KV Store Evaluation | DEP-02..03 | Not started | After HNSW clarifies KV responsibilities |
| 14. Async Engine | ASYNC-01..03 | Not started | Cooperative async Db::run() |
| 15. Knowledge Lifecycle | KL-01..12 | Not started | Extraction, conflict resolution, consolidation (#411) |

**Ongoing (unphased):** QUAL-01..07, ITEST-01..06, PERF-V2-01..03, CAP-01..02

## Accumulated Context

### Decisions (v2)

- AD-23: v2 requirements consolidated from issues #405, #408, #409 into planning docs as canon
- AD-24: Bug fixes (Phase 9) must precede recall pipeline — correctness before features
- AD-25: Typed query builder sequenced after recall pipeline — safety layer before patterns multiply, but recall pipeline lights up the product
- AD-26: graph-builder absorption into mneme during HNSW redesign — kills rayon pin and crate boundary simultaneously
- AD-27: RocksDB evaluation deferred until HNSW redesign clarifies actual KV store responsibilities
- AD-28: Integration tests built alongside features, not as separate phase
- AD-29: Quality cleanup is opportunistic — touch the file, fix the lint. Never scheduled standalone.
- AD-30: Knowledge lifecycle (extraction + conflict resolution) sequenced after recall pipeline and typed query builder — extraction stage lives adjacent to RecallStage, writes need schema safety. Consolidation/forgetting deferred until graph actually grows.

### Key References

- ABSORPTION.md: `docs/ABSORPTION.md` — v1 audit trail (lines removed, unsafe sites, unwraps)
- KnowledgeStore: `crates/mneme/src/knowledge_store.rs`
- RecallStage: `crates/nous/src/recall.rs`
- HNSW implementation: `crates/mneme-engine/src/runtime/hnsw.rs`
- Eval loop: `crates/mneme-engine/src/query/eval.rs`
- graph-builder: `crates/graph-builder/` (42 unsafe sites, rayon pin)
- Hybrid search: `search_hybrid()` in KnowledgeStore

### Blockers/Concerns

- R8: Graph score aggregation bug (BUG-01) — must fix before recall pipeline ships
- R9: RRF rank encoding ambiguity (BUG-02) — 0 vs -1 for missing signals
- R10: Rayon pin (DEP-01) — ticking clock, resolved during HNSW redesign

## Session Continuity

Last session: 2026-03-03
Stopped at: v2 planning docs integrated. Issues #405, #408, #409, #411 ready to close.
Resume file: Phase 9 (Bug Fixes) when ready to start executing

---
*Last updated: 2026-03-03 — v2 planning integrated from issues #405, #408, #409, #411*
