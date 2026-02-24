---
phase: 02-orchestrator-and-entry
plan: "01"
subsystem: planning
tags: [dianoia, orchestrator, event-bus, sqlite, planning-fsm]

# Dependency graph
requires:
  - phase: 01-foundation
    provides: PlanningStore, DianoiaFSM (transition), PlanningConfigSchema in taxis/schema.ts, planning DDL migration v20
provides:
  - DianoiaOrchestrator class with handle(), abandon(), confirmResume(), getActiveProject(), hasPendingConfirmation()
  - SessionStore.getDb() public accessor enabling db injection into PlanningStore
  - EventName union extended with five planning: events (type-safe event bus)
  - VALID_EVENTS set extended with same five events (YAML hook validation)
  - RuntimeServices.planningOrchestrator optional field
  - NousManager.setPlanningOrchestrator() setter wired into buildServices()
  - Planning context block injection in context.ts system prompt
  - aletheia.ts createRuntime() instantiates DianoiaOrchestrator from store.getDb()
affects:
  - 02-02 (plan command tool calls orchestrator.handle())
  - 02-03 (turn:before handler reads planningOrchestrator from services)
  - 03-integration (fires planning:project-created, planning:project-resumed events)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - Setter pattern on NousManager for optional services (setX then wired in buildServices())
    - Soft presence pattern for system prompt injection (only when active project exists)
    - cast-through-unknown for PlanningConfig extension fields (pendingConfirmation flag)
    - Orchestrator as thin coordination layer over PlanningStore + FSM + eventBus

key-files:
  created:
    - infrastructure/runtime/src/dianoia/orchestrator.ts
  modified:
    - infrastructure/runtime/src/mneme/store.ts
    - infrastructure/runtime/src/koina/event-bus.ts
    - infrastructure/runtime/src/koina/hooks.ts
    - infrastructure/runtime/src/dianoia/index.ts
    - infrastructure/runtime/src/nous/pipeline/types.ts
    - infrastructure/runtime/src/nous/manager.ts
    - infrastructure/runtime/src/nous/pipeline/stages/context.ts
    - infrastructure/runtime/src/aletheia.ts

key-decisions:
  - "handle() and abandon() are sync (not async) — no await in body; confirmResume() is also sync for same reason; future plans can change signature if needed"
  - "pendingConfirmation flag stored in project.config JSON via cast-through-unknown — avoids schema change for ephemeral confirmation state"
  - "Planning context injection placed after working-state block, before post-distillation priming — ensures planning state is fresh in system prompt without displacing distillation context"
  - "DianoiaOrchestrator instantiated in createRuntime() (not startRuntime()) so it is available in tests and CLI commands that call createRuntime() directly"

patterns-established:
  - "Setter pattern: private field + setX() method on NousManager, then spread into buildServices()"
  - "Soft presence: system prompt block only injected when services.planningOrchestrator?.getActiveProject() returns a result"

requirements-completed:
  - ENTRY-03
  - ENTRY-04

# Metrics
duration: 3min
completed: 2026-02-24
---

# Phase 2 Plan 1: DianoiaOrchestrator Wiring Summary

**DianoiaOrchestrator wired as single planning state driver: handle()/confirmResume()/abandon() on top of PlanningStore + FSM, injected into RuntimeServices and system prompt**

## Performance

- **Duration:** 3 min
- **Started:** 2026-02-24T00:00:24Z
- **Completed:** 2026-02-24T00:03:18Z
- **Tasks:** 2
- **Files modified:** 8 (plus 1 created)

## Accomplishments

- Three unblocking changes in a single commit: `SessionStore.getDb()` accessor, five `planning:*` events added to `EventName` union, same five events mirrored in `VALID_EVENTS` set
- `DianoiaOrchestrator` class created with all five required methods; wired into `NousManager` via setter pattern and propagated through `buildServices()` to `RuntimeServices`
- Planning context injected into system prompt on every turn where an active project exists for the current nous

## Task Commits

1. **Task 1: Unblock wiring — SessionStore.getDb(), planning EventNames, VALID_EVENTS** - `5776131` (feat)
2. **Task 2: DianoiaOrchestrator, RuntimeServices, aletheia.ts wiring, context.ts injection** - `41ae38b` (feat)

## Files Created/Modified

- `infrastructure/runtime/src/dianoia/orchestrator.ts` - DianoiaOrchestrator class (new)
- `infrastructure/runtime/src/mneme/store.ts` - Added `getDb(): Database.Database` public accessor
- `infrastructure/runtime/src/koina/event-bus.ts` - Extended EventName union with five planning: events
- `infrastructure/runtime/src/koina/hooks.ts` - Extended VALID_EVENTS set with same five events
- `infrastructure/runtime/src/dianoia/index.ts` - Added DianoiaOrchestrator barrel export
- `infrastructure/runtime/src/nous/pipeline/types.ts` - Added planningOrchestrator optional field to RuntimeServices
- `infrastructure/runtime/src/nous/manager.ts` - Added planningOrchestrator private field, setter, buildServices() spread
- `infrastructure/runtime/src/nous/pipeline/stages/context.ts` - Planning context block injection
- `infrastructure/runtime/src/aletheia.ts` - Instantiate DianoiaOrchestrator in createRuntime()

## Decisions Made

- `handle()` and `abandon()` are synchronous (no `await` in body). oxlint `require-await` would flag them as async with no await. The sync signature is fine now; Plans 02-02 and 02-03 call them directly.
- `pendingConfirmation` flag is stored in `project.config` JSON using cast-through-unknown. This avoids a schema migration for ephemeral confirmation state that never needs to be queried.
- Orchestrator is instantiated in `createRuntime()` (not `startRuntime()`) so CLI commands and unit tests that call `createRuntime()` also get planning support.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed double-cast TypeScript error on PlanningConfigSchema spread**
- **Found during:** Task 2 (DianoiaOrchestrator creation)
- **Issue:** Spreading `{ ...config, pendingConfirmation: true }` and casting directly to `PlanningConfigSchema` is an overlap error — TS requires cast through `unknown` when types don't sufficiently overlap
- **Fix:** Changed `as PlanningConfigSchema` to `as unknown as PlanningConfigSchema` in two places in orchestrator.ts
- **Files modified:** `infrastructure/runtime/src/dianoia/orchestrator.ts`
- **Verification:** `npx tsc --noEmit` passes clean
- **Committed in:** `41ae38b` (Task 2 commit)

**2. [Rule 1 - Bug] Removed spurious async keywords from handle() and abandon()**
- **Found during:** Task 2 lint pass (oxlint `require-await`)
- **Issue:** Both methods were declared async but contained no await — oxlint correctly flagged them as warnings
- **Fix:** Removed `async`/`Promise` wrappers; changed `await this.abandon()` call in `confirmResume()` to `this.abandon()`
- **Files modified:** `infrastructure/runtime/src/dianoia/orchestrator.ts`
- **Verification:** Zero warnings from `npx oxlint src/dianoia/orchestrator.ts`
- **Committed in:** `41ae38b` (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (both Rule 1 — TypeScript/lint correctness)
**Impact on plan:** Both fixes necessary for type-safety and lint compliance. No scope creep.

## Issues Encountered

None beyond the auto-fixed deviations above.

## Next Phase Readiness

- `orchestrator.handle(nousId, sessionId)` is the single entry point for all planning commands — Plans 02-02 and 02-03 can call it directly without additional wiring
- `services.planningOrchestrator` is available in all pipeline stages via `RuntimeServices`
- Five planning events are type-safe in both TypeScript (EventName union) and YAML hooks (VALID_EVENTS)
- 76 dianoia tests pass (60 FSM + 16 store) — no regressions

---
*Phase: 02-orchestrator-and-entry*
*Completed: 2026-02-24*
