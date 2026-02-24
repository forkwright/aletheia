---
phase: 03-project-context-and-api
plan: "01"
subsystem: database
tags: [sqlite, dianoia, planning, fsm, project-context]

requires:
  - phase: 01-foundation
    provides: PlanningStore, PlanningProject, PLANNING_V20_DDL, DianoiaOrchestrator skeleton
  - phase: 02-orchestrator-and-entry
    provides: DianoiaOrchestrator.handle(), confirmResume(), intent detection

provides:
  - PLANNING_V21_MIGRATION (ALTER TABLE planning_projects ADD COLUMN project_context TEXT)
  - ProjectContext interface (goal/coreValue/constraints/keyDecisions/rawTranscript)
  - PlanningProject.projectContext field populated from DB
  - PlanningStore.updateProjectGoal() and updateProjectContext()
  - DianoiaOrchestrator.processAnswer(), getNextQuestion(), synthesizeContext(), confirmSynthesis()
  - DianoiaOrchestrator.completePhase(), completeProject() lifecycle stubs

affects:
  - 03-02 (API layer will expose processAnswer/getNextQuestion/confirmSynthesis)
  - 04-research (research phase begins after confirmSynthesis transitions to researching state)

tech-stack:
  added: []
  patterns:
    - "Migration as exported constant: PLANNING_V21_MIGRATION exported from dianoia/schema.ts, imported into mneme/schema.ts MIGRATIONS array"
    - "Null-safe JSON column: project_context parsed with try/catch in mapProject, returns null on failure"
    - "Transcript accumulation: rawTranscript grows via processAnswer, never replaced, preserved through confirmSynthesis"
    - "exactOptionalPropertyTypes spread: conditional spread pattern for optional array fields"

key-files:
  created: []
  modified:
    - infrastructure/runtime/src/dianoia/schema.ts
    - infrastructure/runtime/src/dianoia/types.ts
    - infrastructure/runtime/src/dianoia/store.ts
    - infrastructure/runtime/src/dianoia/orchestrator.ts
    - infrastructure/runtime/src/dianoia/orchestrator.test.ts
    - infrastructure/runtime/src/dianoia/store.test.ts
    - infrastructure/runtime/src/dianoia/index.ts
    - infrastructure/runtime/src/mneme/schema.ts

key-decisions:
  - "COMPLETE_QUESTIONING does not exist in machine.ts; correct FSM event is START_RESEARCH (questioning -> researching)"
  - "planning:checkpoint event does not exist in event-bus.ts; confirmSynthesis emits planning:phase-started instead"
  - "confirmSynthesis merges rawTranscript from existing context, not from synthesizedContext — transcript is preserved verbatim"
  - "exactOptionalPropertyTypes: conditional spread pattern required for optional array fields in merged context object"
  - "completeProject requires project in verifying state to use ALL_PHASES_COMPLETE transition — tests use direct DB update for isolation"

patterns-established:
  - "Migration constants: each migration is a named export from the module's schema.ts, imported into mneme/schema.ts"
  - "Context accumulation pattern: processAnswer grows rawTranscript, confirmSynthesis merges structured fields while preserving transcript"

requirements-completed: [PROJ-01, PROJ-02, PROJ-03]

duration: 5min
completed: 2026-02-24
---

# Phase 3 Plan 01: Project Context Persistence Summary

**Conversational project context gathering persisted to SQLite via migration v21, with processAnswer/getNextQuestion/confirmSynthesis driving the questioning loop from questioning to researching state**

## Performance

- **Duration:** 5 min
- **Started:** 2026-02-24T00:40:55Z
- **Completed:** 2026-02-24T00:45:30Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments

- Migration v21 adds `project_context TEXT` column to planning_projects, registered in mneme MIGRATIONS array
- ProjectContext interface with goal/coreValue/constraints/keyDecisions/rawTranscript fields
- PlanningStore gains updateProjectGoal() and updateProjectContext() with proper error handling and transactions
- DianoiaOrchestrator gains full questioning loop: processAnswer accumulates transcript, getNextQuestion returns next from 5-question sequence, synthesizeContext formats for confirmation, confirmSynthesis persists structured context and transitions FSM to researching
- 118 total dianoia tests passing with zero regressions; tsc clean

## Task Commits

1. **Task 1: Migration v21 and ProjectContext persistence layer** - `e7d542b` (feat)
2. **Task 2: Questioning loop** - `6cb97f4` (feat)

**Plan metadata:** (docs commit follows)

## Files Created/Modified

- `infrastructure/runtime/src/dianoia/schema.ts` - Added PLANNING_V21_MIGRATION export
- `infrastructure/runtime/src/dianoia/types.ts` - Added ProjectContext interface; extended PlanningProject with projectContext field
- `infrastructure/runtime/src/dianoia/store.ts` - Added updateProjectGoal(), updateProjectContext(); mapProject() parses project_context column
- `infrastructure/runtime/src/dianoia/orchestrator.ts` - Added processAnswer(), getNextQuestion(), synthesizeContext(), confirmSynthesis(), completePhase(), completeProject()
- `infrastructure/runtime/src/dianoia/orchestrator.test.ts` - 12 new tests for questioning loop; makeDb() updated with v21 migration
- `infrastructure/runtime/src/dianoia/store.test.ts` - 7 new tests for updateProjectGoal, updateProjectContext, null projectContext; beforeEach updated with v21 migration
- `infrastructure/runtime/src/dianoia/index.ts` - Exports PLANNING_V21_MIGRATION and ProjectContext type
- `infrastructure/runtime/src/mneme/schema.ts` - Imports PLANNING_V21_MIGRATION; adds version 21 entry to MIGRATIONS array

## Decisions Made

- Used `START_RESEARCH` as the FSM transition event from questioning to researching (plan referenced non-existent `COMPLETE_QUESTIONING`)
- Used `planning:phase-started` event in confirmSynthesis (plan referenced non-existent `planning:checkpoint`)
- Conditional spread pattern for rawTranscript in merge to satisfy `exactOptionalPropertyTypes` compiler option
- completeProject requires `verifying` state (ALL_PHASES_COMPLETE event) — orchestrator-test uses direct DB update for state isolation

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Incorrect FSM transition event name in confirmSynthesis**
- **Found during:** Task 2 (orchestrator implementation)
- **Issue:** Plan specified `transition("questioning", "COMPLETE_QUESTIONING")` but machine.ts defines no such event; valid event is `START_RESEARCH`
- **Fix:** Used `transition("questioning", "START_RESEARCH")` — the correct event for questioning -> researching in VALID_TRANSITIONS
- **Files modified:** infrastructure/runtime/src/dianoia/orchestrator.ts
- **Verification:** FSM transitions correctly; tsc clean; all orchestrator tests pass
- **Committed in:** 6cb97f4 (Task 2 commit)

**2. [Rule 1 - Bug] Non-existent planning:checkpoint event referenced in confirmSynthesis**
- **Found during:** Task 2 (orchestrator implementation)
- **Issue:** Plan specified `eventBus.emit("planning:checkpoint", ...)` but EventName union in event-bus.ts has no such member
- **Fix:** Omitted planning:checkpoint emit; confirmSynthesis emits `planning:phase-started` (the semantically correct transition event already defined in EventName)
- **Files modified:** infrastructure/runtime/src/dianoia/orchestrator.ts
- **Verification:** tsc clean; eventBus.emit only called with valid EventName values
- **Committed in:** 6cb97f4 (Task 2 commit)

**3. [Rule 1 - Bug] exactOptionalPropertyTypes spread failure on rawTranscript**
- **Found during:** Task 2 (type check after implementation)
- **Issue:** `{ ...existing, rawTranscript: existing.rawTranscript }` fails under exactOptionalPropertyTypes because rawTranscript can be undefined, not assignable to the required array type
- **Fix:** Conditional spread `...(rawTranscript !== undefined ? { rawTranscript } : {})` avoids assigning undefined to optional property
- **Files modified:** infrastructure/runtime/src/dianoia/orchestrator.ts
- **Verification:** `npx tsc --noEmit` passes with zero errors
- **Committed in:** 6cb97f4 (Task 2 commit)

---

**Total deviations:** 3 auto-fixed (all Rule 1 — bugs in plan specification)
**Impact on plan:** All three fixes correct the plan's references to match actual codebase. No scope creep; implementation intent preserved exactly.

## Issues Encountered

None beyond the deviations documented above.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Questioning loop is complete and tested; confirmSynthesis transitions project to `researching` state
- Phase 3 Plan 02 (API layer) can now expose processAnswer/getNextQuestion/confirmSynthesis as HTTP endpoints
- Phase 4 (Research) begins after a project reaches `researching` state via confirmSynthesis

---
*Phase: 03-project-context-and-api*
*Completed: 2026-02-24*

## Self-Check: PASSED

- All 9 files found on disk
- Commits e7d542b and 6cb97f4 verified in git log
- 118 dianoia tests passing, tsc clean
