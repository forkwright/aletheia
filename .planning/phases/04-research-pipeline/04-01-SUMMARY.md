---
phase: 04-research-pipeline
plan: "01"
subsystem: database
tags: [sqlite, migration, sessions_dispatch, parallel-agents, planning, vitest]

requires:
  - phase: 02-orchestrator-and-entry
    provides: DianoiaOrchestrator with getProject() and store.getDb() accessors
  - phase: 03-project-context-and-api
    provides: PlanningStore createResearch(), PlanningResearch type, sessions_dispatch ToolHandler

provides:
  - PLANNING_V22_MIGRATION — status column on planning_research table
  - ResearchOrchestrator — dispatches 4 parallel researchers via sessions_dispatch
  - plan_research ToolHandler — registered in aletheia.ts for agent invocation
  - researcher.test.ts — 3 tests covering success/partial/failed dimension paths

affects:
  - 04-02-synthesis (reads research rows with status field to decide skip/timeout handling)
  - 05-requirements (consumes research summaries stored by ResearchOrchestrator)

tech-stack:
  added: []
  patterns:
    - ResearchOrchestrator takes (db, dispatchTool) and creates its own PlanningStore internally
    - context field (not ephemeralSoul) used for dimension soul injection in sessions_dispatch tasks
    - DispatchTask status mapped to planning_research.status (success->complete, timeout->partial, error->failed)

key-files:
  created:
    - infrastructure/runtime/src/dianoia/researcher.ts
    - infrastructure/runtime/src/dianoia/research-tool.ts
    - infrastructure/runtime/src/dianoia/researcher.test.ts
  modified:
    - infrastructure/runtime/src/dianoia/schema.ts
    - infrastructure/runtime/src/dianoia/types.ts
    - infrastructure/runtime/src/dianoia/store.ts
    - infrastructure/runtime/src/dianoia/index.ts
    - infrastructure/runtime/src/mneme/schema.ts
    - infrastructure/runtime/src/aletheia.ts
    - infrastructure/runtime/src/dianoia/store.test.ts

key-decisions:
  - "ResearchOrchestrator constructor takes (db, dispatchTool) and builds its own PlanningStore — matches DianoiaOrchestrator pattern, avoids exposing store"
  - "context field used for soul injection (not ephemeralSoul — that param does not exist on sessions_dispatch DispatchTask interface)"
  - "status column has DEFAULT 'complete' so existing rows retain backward compatibility without data migration"
  - "plan_research tool skip branch returns early with {status: skipped} — skipResearch() deferred to plan 04-02"
  - "store.test.ts auto-fixed to include V22 migration — createResearch() now inserts status column which v22 adds"

patterns-established:
  - "Orchestrator internal pattern: class takes (db, dependencyTool), creates own PlanningStore(db)"
  - "Dispatch result mapping: success->complete, timeout->partial, error->failed stored in status column"
  - "Test makeDb() must include all migrations through current version to avoid column-not-found errors"

requirements-completed:
  - RESR-01
  - RESR-02
  - RESR-05

duration: 3min
completed: 2026-02-24
---

# Phase 4 Plan 01: Research Pipeline Foundation Summary

**SQLite migration v22 (status column), ResearchOrchestrator dispatching 4 dimension agents via sessions_dispatch, and plan_research tool registered in aletheia.ts**

## Performance

- **Duration:** 3 min
- **Started:** 2026-02-24T01:20:46Z
- **Completed:** 2026-02-24T01:23:27Z
- **Tasks:** 3
- **Files modified:** 9

## Accomplishments

- Migration v22 adds status TEXT column to planning_research with CHECK constraint (complete/partial/failed)
- ResearchOrchestrator dispatches 4 parallel tasks (stack, features, architecture, pitfalls) via sessions_dispatch, storing results per-dimension with correct status
- plan_research tool registered in aletheia.ts; called after dispatchTool is available (correct ordering)
- 3 tests in researcher.test.ts covering all result status paths — all pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Migration v22 (status column on planning_research)** - `b11a1f7` (feat)
2. **Task 2: ResearchOrchestrator class with dimension souls and dispatch wiring** - `3619c78` (feat)
3. **Task 3: plan_research tool and aletheia.ts wiring** - `3232798` (feat)

## Files Created/Modified

- `infrastructure/runtime/src/dianoia/schema.ts` — Added PLANNING_V22_MIGRATION export
- `infrastructure/runtime/src/dianoia/types.ts` — Added status field to PlanningResearch interface
- `infrastructure/runtime/src/dianoia/store.ts` — createResearch() accepts status, mapResearch() reads it
- `infrastructure/runtime/src/mneme/schema.ts` — Migration v22 registered in MIGRATIONS array
- `infrastructure/runtime/src/dianoia/researcher.ts` — ResearchOrchestrator with DIMENSIONS, DIMENSION_SOULS, runResearch()
- `infrastructure/runtime/src/dianoia/researcher.test.ts` — 3 tests covering success/partial/failed paths
- `infrastructure/runtime/src/dianoia/research-tool.ts` — createPlanResearchTool() ToolHandler
- `infrastructure/runtime/src/dianoia/index.ts` — Exports ResearchOrchestrator, createPlanResearchTool, PLANNING_V22_MIGRATION
- `infrastructure/runtime/src/aletheia.ts` — Imports and registers plan_research tool after dispatchTool
- `infrastructure/runtime/src/dianoia/store.test.ts` — Auto-fixed to include V22 migration in makeDb()

## Decisions Made

- `context` field used for soul injection (not `ephemeralSoul` — that param does not exist on `DispatchTask` interface in sessions-dispatch.ts)
- `status` column DEFAULT is `'complete'` — backward compatible with existing rows, no data migration needed
- `plan_research` skip branch returns early with `{status: "skipped"}` — `skipResearch()` deferred to plan 04-02 per design
- `ResearchOrchestrator` placed after `dispatchTool` in `aletheia.ts` createRuntime() to satisfy ordering constraint

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] store.test.ts makeDb() missing V22 migration**
- **Found during:** Task 3 (plan_research tool and aletheia.ts wiring)
- **Issue:** Running full dianoia test suite revealed store.test.ts fails after Task 1 changes: createResearch() now INSERTs into status column which only exists after V22 migration, but store.test.ts makeDb() only applied V20+V21
- **Fix:** Added PLANNING_V22_MIGRATION import and `db.exec(PLANNING_V22_MIGRATION)` to beforeEach() in store.test.ts
- **Files modified:** infrastructure/runtime/src/dianoia/store.test.ts
- **Verification:** All 121 tests pass after fix
- **Committed in:** `3232798` (Task 3 commit)

---

**Total deviations:** 1 auto-fixed (Rule 1 — bug in existing test infrastructure caused by Task 1 schema change)
**Impact on plan:** Required fix; no scope creep. store.test.ts was broken by V22 migration adding a NOT NULL column that createResearch() now always writes.

## Issues Encountered

None beyond the auto-fixed store.test.ts issue above.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- ResearchOrchestrator.runResearch() is ready for plan 04-02 synthesis layer
- planning_research rows now carry status field for 04-02 to surface timeout/failed dimensions
- plan_research tool is registered and callable by the nous after project enters researching state
- DianoiaOrchestrator.skipResearch() method needs to be added in plan 04-02 to complete skip flow

## Self-Check: PASSED

All files created: researcher.ts, research-tool.ts, researcher.test.ts, 04-01-SUMMARY.md
All commits found: b11a1f7, 3619c78, 3232798

---
*Phase: 04-research-pipeline*
*Completed: 2026-02-24*
