---
phase: 07-execution-orchestration
plan: "01"
subsystem: dianoia
tags: [execution, wave-computation, dependency-graph, spawn-records, tdd, sqlite-migration]
dependency_graph:
  requires: [dianoia/store, dianoia/roadmap, koina/errors, mneme/schema]
  provides: [ExecutionOrchestrator, computeWaves, directDependents, findResumeWave, planning_spawn_records table]
  affects: [dianoia/execution, dianoia/store, dianoia/types, dianoia/schema, mneme/schema, koina/error-codes]
tech_stack:
  added: [PLANNING_V24_MIGRATION, SpawnRecord type, PLANNING_SPAWN_NOT_FOUND error code]
  patterns: [TDD red-green-fix, wave-based parallel dispatch, crash-safe pre-dispatch record creation, direct-dependents-only cascade skip]
key_files:
  created:
    - infrastructure/runtime/src/dianoia/execution.ts
    - infrastructure/runtime/src/dianoia/execution.test.ts
  modified:
    - infrastructure/runtime/src/dianoia/schema.ts
    - infrastructure/runtime/src/mneme/schema.ts
    - infrastructure/runtime/src/dianoia/store.ts
    - infrastructure/runtime/src/dianoia/types.ts
    - infrastructure/runtime/src/koina/error-codes.ts
decisions:
  - "ExecutionOrchestrator takes (db, dispatchTool) with db not stored as private field — passed to PlanningStore constructor only, avoids TS6138 unused-property error"
  - "PLANNING_SPAWN_NOT_FOUND added to error-codes.ts alongside other PLANNING_* codes — not inlined as string"
  - "computeWaves uses PhasePlan.dependencies (plan-to-plan), not PlanStep.dependsOn (step-to-step) as the unit of parallelism"
  - "Cascade-skip is direct-dependents-only: Plan A fails skips B (B depends A), but C (depends B) continues unless B also fails"
  - "Spawn records created BEFORE dispatch so crash-before-dispatch leaves a recoverable trace"
  - "zombie threshold is 600 seconds (2x the 300s plan timeout) for reap-zombie logic"
metrics:
  duration: 10 min
  completed: 2026-02-24
  tasks_completed: 3
  files_changed: 7
---

# Phase 7 Plan 01: ExecutionOrchestrator Foundation Summary

Wave-based execution engine with dependency graph computation, crash-safe spawn records (V24 SQLite migration), and cascade-skip logic — all TDD-driven with 14 passing unit tests.

## What Was Built

### V24 Migration — `planning_spawn_records` table
Added `PLANNING_V24_MIGRATION` to `dianoia/schema.ts` with:
- `planning_spawn_records` table tracking per-plan execution state (6 statuses: pending/running/done/failed/skipped/zombie)
- Two indexes: `idx_spawn_records_project` (project + wave lookup) and `idx_spawn_records_phase` (phase + status lookup)
- Wired into `mneme/schema.ts` MIGRATIONS array at version 24

### `SpawnRecord` type — `dianoia/types.ts`
New interface with all fields including `sessionKey`, `errorMessage`, `partialOutput`, `startedAt`, `completedAt`.

### Store methods — `dianoia/store.ts`
Four new methods on `PlanningStore`:
- `createSpawnRecord(opts)` — inserts pending record, returns mapped SpawnRecord
- `getSpawnRecordOrThrow(id)` — throws `PLANNING_SPAWN_NOT_FOUND` if missing
- `updateSpawnRecord(id, updates)` — dynamic SET construction for partial updates
- `listSpawnRecords(projectId, phaseId?)` — ordered by wave_number, created_at
- `getDb()` — public accessor for db reference

### `ExecutionOrchestrator` — `dianoia/execution.ts`
Exported functions and class:
- `computeWaves(phases)` — groups independent plans into concurrent waves; handles cycles by treating remaining as one wave
- `directDependents(failedPhaseId, allPhases)` — returns only immediate dependents (not transitive)
- `findResumeWave(records)` — returns first wave with incomplete records, -1 if all done, 0 if no records
- `ExecutionOrchestrator` class with `executePhase()`, `getExecutionSnapshot()`, private `reapZombies()`, `isPaused()`

### Error code
Added `PLANNING_SPAWN_NOT_FOUND` to `koina/error-codes.ts`.

## Test Results

14 tests, 4 describe blocks, all passing:

| Suite | Tests | Description |
|-------|-------|-------------|
| computeWaves | 4 | independent grouping, single plan, cycle detection, linear chain |
| directDependents | 3 | direct only, empty when no match, multiple dependents |
| findResumeWave | 4 | empty records, all done, first incomplete, all skipped |
| PlanningStore spawn records | 3 | create+retrieve, update fields, list |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Type Error] Fixed unused `db` private parameter in ExecutionOrchestrator**
- **Found during:** Step 7 (type check)
- **Issue:** `private db: Database.Database` in constructor params caused TS6138 (property declared but never read) since `db` is only passed to `PlanningStore` constructor
- **Fix:** Removed `private` keyword from `db` parameter — passed to `PlanningStore` at construction, not stored as instance field
- **Files modified:** `infrastructure/runtime/src/dianoia/execution.ts`
- **Commit:** e68f47f

**2. [Rule 2 - Missing Error Code] Added `PLANNING_SPAWN_NOT_FOUND` error code**
- **Found during:** Step 7 (type check)
- **Issue:** `"PLANNING_NOT_FOUND"` used in `getSpawnRecordOrThrow` was not a valid `ErrorCode` — TS2820
- **Fix:** Added `PLANNING_SPAWN_NOT_FOUND` to `koina/error-codes.ts` and updated the store usage
- **Files modified:** `infrastructure/runtime/src/koina/error-codes.ts`, `infrastructure/runtime/src/dianoia/store.ts`
- **Commit:** e68f47f

## Self-Check: PASSED

All created files exist. All 3 task commits verified. PLANNING_V24_MIGRATION present in schema.ts. V24 wired in mneme/schema.ts. createSpawnRecord/updateSpawnRecord/listSpawnRecords in store.ts. computeWaves/directDependents/findResumeWave/ExecutionOrchestrator exported from execution.ts. 14/14 tests pass. npx tsc --noEmit clean.
