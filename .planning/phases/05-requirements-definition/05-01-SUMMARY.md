---
phase: 05-requirements-definition
plan: "01"
subsystem: database
tags: [sqlite, migrations, planning, requirements]

requires:
  - phase: 04-research-pipeline
    provides: PlanningStore with research rows; v22 migration applied to planning_research

provides:
  - PLANNING_V23_MIGRATION constant (ALTER TABLE planning_requirements ADD COLUMN rationale TEXT)
  - version 23 entry in mneme/schema.ts MIGRATIONS array
  - rationale: string | null field on PlanningRequirement interface
  - PlanningStore.createRequirement() accepts and persists optional rationale
  - PlanningStore.updateRequirement(id, {tier?, rationale?}) with transaction and NOT_FOUND guard
  - PLANNING_REQUIREMENT_NOT_FOUND error code
  - 6 new unit tests covering rationale create/update behaviors

affects:
  - 05-requirements-definition (RequirementsOrchestrator in Plan 05-02 uses updateRequirement and rationale field)
  - any future plan reading PlanningRequirement (new rationale field)

tech-stack:
  added: []
  patterns:
    - "updateRequirement follows same dynamic-SET pattern as updatePhaseStatus — build sets/vals arrays, transaction-wrap, check result.changes === 0"
    - "mapRequirement reads rationale directly from row with ?? null fallback — same pattern as phaseId"

key-files:
  created: []
  modified:
    - infrastructure/runtime/src/dianoia/schema.ts
    - infrastructure/runtime/src/mneme/schema.ts
    - infrastructure/runtime/src/dianoia/types.ts
    - infrastructure/runtime/src/dianoia/store.ts
    - infrastructure/runtime/src/koina/error-codes.ts
    - infrastructure/runtime/src/dianoia/store.test.ts
    - infrastructure/runtime/src/dianoia/orchestrator.test.ts
    - infrastructure/runtime/src/dianoia/researcher.test.ts

key-decisions:
  - "PLANNING_REQUIREMENT_NOT_FOUND added to error-codes.ts alongside other PLANNING_* codes — not inlined as string literal"
  - "updateRequirement uses dynamic SET construction (sets[]/vals[] arrays) — allows updating tier-only, rationale-only, or both atomically in one UPDATE statement"
  - "rationale field placed after status in PlanningRequirement interface, comment marks it as meaningful only when tier is out-of-scope"
  - "createRequirement INSERT always passes rationale column (even if null) — avoids schema-dependent conditional column list"

patterns-established:
  - "Dynamic-SET update pattern: accumulate SET clauses and values, then execute single UPDATE — reuse for any future partial-update store methods"

requirements-completed:
  - REQS-04
  - REQS-06

duration: 2min
completed: 2026-02-23
---

# Phase 5 Plan 1: Requirements Definition — Store Foundation Summary

**v23 SQLite migration adds rationale column to planning_requirements; PlanningStore extended with updateRequirement() and rationale-aware createRequirement() for out-of-scope requirement tracking**

## Performance

- **Duration:** ~2 min
- **Started:** 2026-02-23T19:55:00-06:00
- **Completed:** 2026-02-23T19:55:59-06:00
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments

- Added `PLANNING_V23_MIGRATION` constant and registered version 23 in the centralized MIGRATIONS array
- Extended `PlanningRequirement` interface with `rationale: string | null` field (meaningful when tier is out-of-scope)
- Updated `createRequirement()` to accept and persist optional rationale via explicit column list in INSERT
- Added `updateRequirement(id, {tier?, rationale?})` with dynamic SET construction, transaction wrapper, and NOT_FOUND error guard
- Added `PLANNING_REQUIREMENT_NOT_FOUND` to the canonical error code registry
- Updated all three dianoia test file `makeDb()` helpers to include v23; added 6 new unit tests — 130 total pass

## Task Commits

1. **Task 1: v23 migration, type update, and store extension** - `f549c63` (feat)
2. **Task 2: Update test makeDb() helpers and add updateRequirement unit tests** - `ed10dd7` (test)

**Plan metadata:** (this commit)

## Files Created/Modified

- `infrastructure/runtime/src/dianoia/schema.ts` - Added PLANNING_V23_MIGRATION export
- `infrastructure/runtime/src/mneme/schema.ts` - Imported PLANNING_V23_MIGRATION; added version: 23 entry to MIGRATIONS
- `infrastructure/runtime/src/dianoia/types.ts` - Added rationale: string | null to PlanningRequirement interface
- `infrastructure/runtime/src/dianoia/store.ts` - Updated createRequirement() and mapRequirement(); added updateRequirement()
- `infrastructure/runtime/src/koina/error-codes.ts` - Added PLANNING_REQUIREMENT_NOT_FOUND
- `infrastructure/runtime/src/dianoia/store.test.ts` - Added PLANNING_V23_MIGRATION to beforeEach; added 6 new tests
- `infrastructure/runtime/src/dianoia/orchestrator.test.ts` - Updated makeDb() with V22 + V23
- `infrastructure/runtime/src/dianoia/researcher.test.ts` - Updated makeDb() with V23

## Decisions Made

- `PLANNING_REQUIREMENT_NOT_FOUND` added to error-codes.ts alongside other PLANNING_* codes rather than inlining as a string literal — keeps the canonical registry complete.
- `updateRequirement` uses the dynamic-SET pattern (accumulating `sets` and `vals` arrays before a single `UPDATE`) to allow updating tier-only, rationale-only, or both in one atomic statement.
- `createRequirement` INSERT always lists the `rationale` column explicitly (passing `null` when not provided) — avoids conditional column-list logic and is forward-compatible.
- `rationale` placed after `status` in the interface with an inline comment noting it is only meaningful when tier is `out-of-scope` — mirrors the decision documented in CONTEXT.md.

## Deviations from Plan

None — plan executed exactly as written. The only addition beyond the literal plan text was adding `PLANNING_REQUIREMENT_NOT_FOUND` to error-codes.ts, which the plan itself called for in the task action description ("check if this error code already exists... If not, add it").

## Issues Encountered

None.

## User Setup Required

None — no external service configuration required.

## Next Phase Readiness

- `updateRequirement()` and rationale persistence are ready for RequirementsOrchestrator (Plan 05-02)
- All existing dianoia tests continue to pass — zero regressions
- v23 migration is registered and will be applied automatically via the mneme migration runner on next DB open

## Self-Check: PASSED

- FOUND: infrastructure/runtime/src/dianoia/schema.ts (PLANNING_V23_MIGRATION)
- FOUND: infrastructure/runtime/src/mneme/schema.ts (version: 23)
- FOUND: infrastructure/runtime/src/dianoia/types.ts (rationale: string | null)
- FOUND: infrastructure/runtime/src/dianoia/store.ts (updateRequirement)
- FOUND: infrastructure/runtime/src/koina/error-codes.ts (PLANNING_REQUIREMENT_NOT_FOUND)
- FOUND: .planning/phases/05-requirements-definition/05-01-SUMMARY.md
- FOUND commit f549c63 (Task 1)
- FOUND commit ed10dd7 (Task 2)
- 130/130 tests pass, 0 TypeScript errors

---
*Phase: 05-requirements-definition*
*Completed: 2026-02-23*
