---
phase: 01-foundation
plan: "01"
subsystem: database
tags: [sqlite, better-sqlite3, dianoia, planning, migrations]

requires: []

provides:
  - PlanningStore CRUD class for all 5 planning tables (planning_projects, planning_phases, planning_requirements, planning_checkpoints, planning_research)
  - PLANNING_V20_DDL schema constant and migration v20 entry in mneme/schema.ts
  - 5 new PLANNING_* error codes in koina/error-codes.ts
  - PlanningError class in koina/errors.ts
  - DianoiaState type and 6 TypeScript interfaces in dianoia/types.ts

affects:
  - 01-02 (DianoiaOrchestrator wires db into PlanningStore)
  - 01-03 (FSM transitions use PlanningStore.updateProjectState)
  - all phases (every subsequent phase reads/writes via PlanningStore)

tech-stack:
  added: []
  patterns:
    - injected-db pattern (PlanningStore takes Database instance, no internal init)
    - db.transaction() wrapping all multi-step mutations
    - snake_case DB columns mapped to camelCase TypeScript via private mappers
    - JSON columns parsed with PLANNING_STATE_CORRUPT error on malformed data

key-files:
  created:
    - infrastructure/runtime/src/dianoia/schema.ts
    - infrastructure/runtime/src/dianoia/types.ts
    - infrastructure/runtime/src/dianoia/store.ts
    - infrastructure/runtime/src/dianoia/store.test.ts
    - infrastructure/runtime/src/dianoia/index.ts
  modified:
    - infrastructure/runtime/src/koina/error-codes.ts
    - infrastructure/runtime/src/koina/errors.ts
    - infrastructure/runtime/src/mneme/schema.ts

key-decisions:
  - "planning_checkpoints has no updated_at column (append-only, decisions are immutable once recorded)"
  - "contextHash is SHA-256 of goal|nousId|createdAt truncated to 16 hex chars — deterministic but not recomputable without original createdAt"
  - "PlanningStore receives pre-initialized db instance; wiring to SessionStore deferred to Phase 2"
  - "PLANNING_V20_MIGRATION_ENTRY alias exported alongside PLANNING_V20_DDL to document intent"

patterns-established:
  - "injected-db: Store constructors accept Database.Database, do not call db.exec internally"
  - "transaction wrapping: all multi-step mutations use db.transaction() to prevent partial writes"
  - "error mapper: JSON.parse failures in mappers throw PlanningError with PLANNING_STATE_CORRUPT"
  - "OrThrow pattern: getProjectOrThrow / getPhaseOrThrow for required lookups"

requirements-completed:
  - FOUND-01
  - FOUND-03
  - FOUND-04
  - FOUND-05
  - FOUND-06
  - TEST-02

duration: 8min
completed: 2026-02-23
---

# Phase 1 Plan 1: SQLite planning schema, PlanningStore CRUD, and unit tests Summary

**SQLite migration v20 with 5 planning tables, PlanningStore CRUD class with transaction-safe mutations, 16 passing unit tests, and typed PlanningError hierarchy**

## Performance

- **Duration:** 8 min
- **Started:** 2026-02-23T22:20:16Z
- **Completed:** 2026-02-23T22:28:00Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments

- Created `dianoia` module from scratch with schema, types, store, tests, and barrel export
- Added migration v20 to `mneme/schema.ts` MIGRATIONS array importing PLANNING_V20_DDL
- All 5 planning tables use `ON DELETE CASCADE` to prevent orphaned child records
- 16 unit tests covering CRUD, cascade delete, JSON round-trip, corrupt data handling, and transaction isolation

## Task Commits

Each task was committed atomically:

1. **Task 1: Error codes, PlanningError class, types, and schema DDL constant** - `6cc8f58` (feat)
2. **Task 2: PlanningStore class, migration v20 entry, and unit tests** - `4cad5c5` (feat)

## Files Created/Modified

- `infrastructure/runtime/src/dianoia/schema.ts` - PLANNING_V20_DDL with all 5 CREATE TABLE and CREATE INDEX statements
- `infrastructure/runtime/src/dianoia/types.ts` - DianoiaState union and 6 TypeScript interfaces (PlanningProject, PlanningPhase, PlanningConfig, PlanningRequirement, PlanningCheckpoint, PlanningResearch)
- `infrastructure/runtime/src/dianoia/store.ts` - PlanningStore class with 20 methods, all multi-step mutations wrapped in db.transaction()
- `infrastructure/runtime/src/dianoia/store.test.ts` - 16 behavior-focused unit tests using :memory: SQLite
- `infrastructure/runtime/src/dianoia/index.ts` - Barrel export of public API
- `infrastructure/runtime/src/koina/error-codes.ts` - Added 5 PLANNING_* error codes
- `infrastructure/runtime/src/koina/errors.ts` - Added PlanningError subclass with PLANNING_PROJECT_NOT_FOUND as default code
- `infrastructure/runtime/src/mneme/schema.ts` - Added version 20 migration entry importing PLANNING_V20_DDL

## Decisions Made

- `planning_checkpoints` has no `updated_at` because decisions are append-only and immutable once recorded
- `contextHash` is computed at creation time only (SHA-256 of `${goal}|${nousId}|${createdAt}` truncated to 16 hex) — cannot be recomputed later without original `createdAt`
- `PlanningStore` uses injected-db pattern with no internal `init()` call, consistent with planned wiring to `SessionStore` db instance in Phase 2

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `PlanningStore` is fully tested and ready for `DianoiaOrchestrator` (Plan 01-02) to wire the db instance
- FSM transition validation (Plan 01-03) can use `updateProjectState` directly
- All 5 planning tables are live in migration v20 — any instance running the migration runner will have the schema

---
*Phase: 01-foundation*
*Completed: 2026-02-23*

## Self-Check: PASSED

- FOUND: infrastructure/runtime/src/dianoia/schema.ts
- FOUND: infrastructure/runtime/src/dianoia/types.ts
- FOUND: infrastructure/runtime/src/dianoia/store.ts
- FOUND: infrastructure/runtime/src/dianoia/store.test.ts
- FOUND: infrastructure/runtime/src/dianoia/index.ts
- FOUND: .planning/phases/01-foundation/01-01-SUMMARY.md
- FOUND commit 6cc8f58: feat(01-01): error codes, PlanningError class, types, and schema DDL
- FOUND commit 4cad5c5: feat(01-01): PlanningStore CRUD, migration v20 entry, and unit tests
