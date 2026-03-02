---
phase: 08-integrate-idiom-migration
plan: "01"
subsystem: database
tags: [rust, snafu, tracing, cozo, mneme-engine, cherry-pick, gap-closure]

requires:
  - phase: 07-integrate-hybrid-retrieval
    provides: BM25/RRF/search_hybrid on main with Phase 6 conflict resolved

provides:
  - snafu-based error module (crates/mneme-engine/src/error.rs) with BoxErr, DbResult, AdhocError, bail!, ensure!
  - zero miette imports in crates/mneme-engine/src/
  - zero log imports in crates/mneme-engine/src/ (replaced with tracing)
  - zero lazy_static! invocations in crates/mneme-engine/src/ (replaced with LazyLock)
  - env_logger fully removed from mneme-engine (tracing-subscriber used in test harness)
  - unwrap audit complete on public API paths (INVARIANT comments + map_err conversions)
  - docs/ABSORPTION.md documenting full CozoDB absorption status
  - all 53 v1.0 requirements satisfied on main

affects: [milestone-v1.0-complete, feat/mneme-engine PR, all future mneme work]

tech-stack:
  added: [snafu (crate-level error module), tracing-subscriber (test harness only)]
  patterns:
    - "DbResult<T> = std::result::Result<T, BoxErr> as crate-wide error type alias"
    - "bail!/ensure! macros re-exported from lib.rs via #[macro_export]"
    - "LazyLock<BTreeMap<...>> for DEFAULT_FIXED_RULES static initialization"
    - "tracing macros replace all log::debug/info/warn/error call sites"
    - "INVARIANT comments on unwrap() calls that require callers to uphold preconditions"

key-files:
  created:
    - crates/mneme-engine/src/error.rs
    - docs/ABSORPTION.md
  modified:
    - crates/mneme-engine/Cargo.toml
    - crates/mneme-engine/src/lib.rs
    - crates/mneme-engine/src/data/program.rs
    - crates/mneme-engine/src/fixed_rule/mod.rs
    - crates/mneme-engine/src/fixed_rule/utilities/rrf.rs
    - crates/mneme-engine/src/fts/indexing.rs
    - crates/mneme-engine/src/runtime/hnsw.rs
    - crates/mneme-engine/src/runtime/db.rs
    - crates/mneme-engine/src/runtime/relation.rs
    - crates/mneme-engine/src/storage/mem.rs
    - 53 additional source files (miette->snafu sweep)

key-decisions:
  - "Cherry-picked 8 of 9 p5 commits — skipped daa1024e (docs-only, conflicted with Phase 7/8 gap-closure tracking)"
  - "rrf.rs required manual fix (miette->snafu) since Phase 7 added this file after Phase 5 was implemented"
  - "Squashed all 8 cherry-picks to single commit on main for clean history"
  - "ROADMAP.md conflict from commit 1 resolved by restoring HEAD's version (Phase 8 tracking is authoritative)"

patterns-established:
  - "Gap closure via cherry-pick + squash: proven pattern for integrating diverged feature branches"
  - "4-file auto-merge verification after large miette->snafu commit (data/program.rs, fixed_rule/mod.rs, fts/indexing.rs, runtime/hnsw.rs)"

requirements-completed: [IDIOM-01, IDIOM-02, IDIOM-03, IDIOM-04, IDIOM-05, DOCS-01]

duration: 4min
completed: 2026-03-02
---

# Phase 8 Plan 01: Integrate Idiom Migration Summary

**Migrated 61+ mneme-engine files from miette/log/lazy_static to snafu/tracing/LazyLock via cherry-pick of 8 Phase 5 commits, fixed rrf.rs manually, completing all 53 v1.0 CozoDB absorption requirements**

## Performance

- **Duration:** ~10 min (including test runs)
- **Started:** 2026-03-02T17:57:04Z
- **Completed:** 2026-03-02T18:05:00Z
- **Tasks:** 2 of 2
- **Files modified:** 64 (63 source files + docs/ABSORPTION.md)

## Accomplishments

- Cherry-picked 8 of 9 commits from `feat/mneme-engine-p5` onto main, squashed to a single clean commit
- Manually fixed `rrf.rs` — the one file Phase 5 couldn't reach (Phase 7 added it after Phase 5 was implemented)
- Verified 173 mneme-engine tests pass, 86 mneme tests pass, 5 integration tests pass, clippy clean
- Updated all planning docs: ROADMAP.md, REQUIREMENTS.md, STATE.md — milestone 100% complete
- All 53 v1.0 requirements satisfied on main with zero pending integration

## Task Commits

Each task was committed atomically:

1. **Task 1: Cherry-pick 8 p5 commits, fix rrf.rs, squash to single commit** - `f9f7b8bb` (feat)
2. **Task 2: Update planning docs and reconcile milestone tracking** - `86e7672b` (docs)

## Files Created/Modified

- `crates/mneme-engine/src/error.rs` - snafu-based error module: BoxErr, DbResult, AdhocError, bail!, ensure!
- `docs/ABSORPTION.md` - CozoDB absorption audit: lines removed, unsafe sites, unwraps, cleanup backlog
- `crates/mneme-engine/src/lib.rs` - wires error module, re-exports bail!/ensure! macros
- `crates/mneme-engine/src/fixed_rule/mod.rs` - LazyLock for DEFAULT_FIXED_RULES (preserving RRF from Phase 7)
- `crates/mneme-engine/src/fixed_rule/utilities/rrf.rs` - manual fix: `use miette::Result` -> `use crate::error::DbResult as Result`
- `crates/mneme-engine/src/runtime/hnsw.rs` - INVARIANT comments on unwraps + snafu imports
- 58 additional source files - miette->snafu sweep across all modules

## Decisions Made

- Skipped `daa1024e` (docs-only p5 commit): conflicted with Phase 7/8 gap-closure tracking structure; planning docs updated in Task 2 instead
- `rrf.rs` required manual fix: Phase 7 introduced this file after Phase 5 was implemented, so p5's 61-file miette->snafu sweep never touched it
- ROADMAP.md conflict from commit 1 (696a88f8) resolved by restoring HEAD's version — Phase 8 tracking is authoritative

## Deviations from Plan

None — plan executed exactly as written. The cherry-pick sequence, ROADMAP conflict resolution, and rrf.rs manual fix all proceeded as the research in 08-RESEARCH.md specified.

## Issues Encountered

None. All 8 cherry-picks applied cleanly (auto-merges handled correctly by git). The 4 auto-merged files verified to contain both Phase 7 hybrid retrieval content and Phase 5 snafu imports.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

Milestone v1.0 CozoDB Absorption is complete. All 53 requirements satisfied on main. The `feat/mneme-engine` branch is ready to be opened as a PR.

**Blockers:** None

**Next steps (outside this milestone):**
- Open PR on `feat/mneme-engine` branch with the complete absorption
- Phase 5 cleanup backlog (documented in ABSORPTION.md): unused snafu imports, warn suppressions

---
*Phase: 08-integrate-idiom-migration*
*Completed: 2026-03-02*
