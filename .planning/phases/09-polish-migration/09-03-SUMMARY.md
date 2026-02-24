---
phase: 09-polish-migration
plan: "03"
subsystem: docs, testing, api
tags: [oxlint, tsc, contributing, dianoia, migration, exports]

# Dependency graph
requires:
  - phase: 08-verification-checkpoints
    provides: PLANNING_V24_MIGRATION and PLANNING_V25_MIGRATION in schema.ts; CheckpointSystem and GoalBackwardVerifier implementations
  - phase: 07-execution-orchestration
    provides: ExecutionOrchestrator, spawn records, dianoia planning pipeline

provides:
  - CONTRIBUTING.md Dianoia Module section with 4 documented gotchas
  - PLANNING_V24_MIGRATION and PLANNING_V25_MIGRATION re-exported from dianoia/index.ts
  - Zero-error oxlint output (144 warnings only, pre-existing)
  - Zero-error tsc --noEmit output

affects:
  - PR readiness, future contributors onboarding to dianoia module
  - Consumers importing migration constants from dianoia public API

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "V24/V25 migrations now on dianoia public API — consumers import from index.ts not schema.ts directly"

key-files:
  created: []
  modified:
    - CONTRIBUTING.md
    - infrastructure/runtime/src/dianoia/index.ts
    - infrastructure/runtime/src/dianoia/execution.test.ts
    - infrastructure/runtime/src/dianoia/checkpoint.test.ts

key-decisions:
  - "checkpoint.test.ts uses PlanningStore as type-only (no new PlanningStore) — import type is correct"
  - "Plan described 2 pre-existing oxlint errors; both confirmed: no-duplicates in execution.test.ts and consistent-type-imports in checkpoint.test.ts"

patterns-established:
  - "Migration exports: all PLANNING_VXX_MIGRATION constants re-exported from dianoia/index.ts for clean consumer imports"

requirements-completed:
  - DOCS-03
  - TEST-05
  - TEST-06

# Metrics
duration: 1min
completed: 2026-02-24
---

# Phase 9 Plan 03: Polish & Migration — Docs and Lint Summary

**CONTRIBUTING.md Dianoia Module section (4 gotchas), V24/V25 migration exports from index.ts, and zero-error tsc + oxlint**

## Performance

- **Duration:** 1 min
- **Started:** 2026-02-24T19:25:54Z
- **Completed:** 2026-02-24T19:27:00Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Added `## Dianoia Module` section to CONTRIBUTING.md with module overview, key patterns, and all 4 required gotchas (migration propagation, exactOptionalPropertyTypes, require-await, orchestrator registration)
- Added cross-link from CONTRIBUTING.md to `docs/specs/31_dianoia.md` (both in body and footer of section)
- Fixed `eslint-plugin-import(no-duplicates)` error in `execution.test.ts` by merging two duplicate `import type { ... } from "./types.js"` lines into one
- Fixed `typescript-eslint(consistent-type-imports)` error in `checkpoint.test.ts` by changing `import { PlanningStore }` to `import type { PlanningStore }` (PlanningStore only used as type assertion, never instantiated)
- Added `PLANNING_V24_MIGRATION` and `PLANNING_V25_MIGRATION` to dianoia/index.ts public API exports

## Task Commits

Each task was committed atomically:

1. **Task 1: Update CONTRIBUTING.md with Dianoia Module section** - `273794f` (docs)
2. **Task 2: Fix oxlint errors, add V24/V25 index exports, verify tsc and lint** - `92409fa` (fix)

**Plan metadata:** (pending)

## Files Created/Modified
- `CONTRIBUTING.md` - Added 40-line Dianoia Module section between Code Standards and Reporting Issues
- `infrastructure/runtime/src/dianoia/index.ts` - Added PLANNING_V24_MIGRATION, PLANNING_V25_MIGRATION exports
- `infrastructure/runtime/src/dianoia/execution.test.ts` - Merged duplicate import type from ./types.js
- `infrastructure/runtime/src/dianoia/checkpoint.test.ts` - Changed PlanningStore import to import type

## Decisions Made
- `PlanningStore` in `checkpoint.test.ts` is used only as a type assertion (`as unknown as PlanningStore`); no `new PlanningStore()` calls exist in the file — `import type` is correct, not a false positive
- The plan stated 2 pre-existing oxlint errors; both were confirmed and fixed (no additional errors found)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- `npx tsc --noEmit` exits 0 with no output (clean)
- `npx oxlint src/` shows 0 errors, 144 pre-existing warnings
- CONTRIBUTING.md documents all 4 Dianoia gotchas for contributors
- V24 and V25 migration exports available from dianoia public API
- Ready for remaining Phase 9 plans (integration test, spec doc, status pill UI)

## Self-Check: PASSED

Files verified: CONTRIBUTING.md, index.ts, execution.test.ts, checkpoint.test.ts, 09-03-SUMMARY.md — all present.
Commits verified: 273794f (docs), 92409fa (fix) — both in git log.
Quality gates: tsc --noEmit exits 0; oxlint 0 errors, 144 warnings.

---
*Phase: 09-polish-migration*
*Completed: 2026-02-24*
