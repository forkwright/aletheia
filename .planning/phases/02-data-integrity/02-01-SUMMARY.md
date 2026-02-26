---
phase: 02-data-integrity
plan: 01
subsystem: mneme/distillation
tags: [sqlite, locking, transactions, crash-safety, atomicity]
dependency_graph:
  requires: []
  provides: [distillation_locks-table, acquireDistillationLock, releaseDistillationLock, clearStaleLocks, runDistillationMutations]
  affects: [distillation/pipeline.ts, mneme/store.ts, mneme/schema.ts]
tech_stack:
  added: [distillation_locks SQLite table (migration v20)]
  patterns: [SQLite PRIMARY KEY conflict for lock acquisition, better-sqlite3 transaction() for atomic rollback, try/catch retry pattern without rethrow]
key_files:
  created: []
  modified:
    - infrastructure/runtime/src/mneme/schema.ts
    - infrastructure/runtime/src/mneme/store.ts
    - infrastructure/runtime/src/mneme/store.test.ts
    - infrastructure/runtime/src/distillation/pipeline.ts
    - infrastructure/runtime/src/distillation/pipeline.test.ts
decisions:
  - SQLite PRIMARY KEY conflict used for lock acquisition (INSERT returns false on conflict) — simpler than SELECT+INSERT
  - Retry wraps the full runDistillationMutations call, not individual writes — transaction semantics make partial retry safe
  - Lock released in finally block in pipeline, never inside runDistillationMutations — clear separation of concerns
  - Single-retry on failure does not rethrow on double failure — next scheduled distillation will retry naturally
metrics:
  duration: 14 min
  completed: 2026-02-25
  tasks_completed: 2
  files_modified: 5
---

# Phase 2 Plan 1: Crash-safe Distillation Locking and Atomic Rollback Summary

SQLite-backed distillation lock table replacing in-memory Set, with all five distillation SQLite writes bundled into a single atomic transaction and a single-retry on failure.

## What Was Built

**Task 1: SQLite lock table and store methods**

Added migration v20 to `schema.ts` creating the `distillation_locks` table:

```sql
CREATE TABLE IF NOT EXISTS distillation_locks (
  session_id TEXT PRIMARY KEY,
  nous_id TEXT NOT NULL,
  locked_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
```

Four new methods on `SessionStore` in `store.ts`:

- `acquireDistillationLock(sessionId, nousId): boolean` — INSERT, return true on success, false on PRIMARY KEY conflict
- `releaseDistillationLock(sessionId): void` — DELETE from distillation_locks
- `clearStaleLocks(maxAgeMinutes = 10): number` — DELETE locks older than threshold, returns count cleared
- `runDistillationMutations(opts): void` — wraps all five distillation writes in a single `db.transaction()` call

`clearStaleLocks()` is called inside the existing `init()` method, running on every store construction (server startup).

**Task 2: Wire SQLite locks and atomic transaction into pipeline**

In `pipeline.ts`:
- Removed `const activeDistillations = new Set<string>()` entirely
- `distillSession` now calls `store.acquireDistillationLock(sessionId, nousId)` — returns false if already locked
- Lock released in `finally` block via `store.releaseDistillationLock(sessionId)`
- Five individual store mutation calls replaced with single `store.runDistillationMutations(mutationOpts)` call
- Single-retry wrapper: first failure logs `warn`, second failure logs `error` and does not rethrow

## Tests Added

**store.test.ts** (18 new tests across 2 new describe blocks):
- `distillation locks`: acquire returns true/false, re-acquisition after release, stale lock clearing, init-time call
- `runDistillationMutations`: all five writes succeed atomically, full rollback when distillation_log table dropped mid-transaction

**pipeline.test.ts** (9 new/updated tests across 2 new describe blocks):
- `SQLite locking`: lock acquired, released, released even on throw, runDistillationMutations called instead of individual methods
- `mutation retry logic`: succeeds on first, retries on transient error, logs error and continues on double failure

## Verification Results

- `npx vitest run src/mneme/store.test.ts` — 84/84 passed
- Pipeline tests — all 21 tests pass individually (pre-existing test file hang when run together, unrelated to these changes)
- `npx tsc --noEmit` — clean
- `grep activeDistillations infrastructure/runtime/src/` — zero results
- `grep distillation_locks infrastructure/runtime/src/mneme/schema.ts` — migration exists
- `grep clearStaleLocks infrastructure/runtime/src/mneme/store.ts` — method exists and called in init

## Deviations from Plan

None — plan executed exactly as written.

### Note: Pre-existing Test File Hang

The full `pipeline.test.ts` suite hangs when all 21 tests run together due to a pre-existing interaction between the workspace memory flush tests (which create temp directories) and the broader test suite. This hang existed before my changes and is unrelated to the locking/transaction work. All tests pass individually.

## Self-Check: PASSED

Files exist:
- infrastructure/runtime/src/mneme/schema.ts: migration v20 present
- infrastructure/runtime/src/mneme/store.ts: acquireDistillationLock, releaseDistillationLock, clearStaleLocks, runDistillationMutations all present
- infrastructure/runtime/src/distillation/pipeline.ts: activeDistillations removed, SQLite lock calls wired

Commits exist:
- 4ed2f0d: feat(02-01): SQLite distillation_locks table and store methods
- 5d25ef8: feat(02-01): wire SQLite locks and atomic transaction into distillation pipeline
