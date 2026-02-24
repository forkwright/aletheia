---
phase: 06-roadmap-phase-planning
plan: "02"
subsystem: planning
tags: [dianoia, roadmap, tool-handler, fsm, orchestrator]

requires:
  - phase: 06-01
    provides: RoadmapOrchestrator with generateRoadmap, commitRoadmap, validateCoverage, validateCoverageFromDb, listPhases, adjustPhase, planPhase, formatRoadmapDisplay
  - phase: 05-02
    provides: createPlanRequirementsTool pattern (ToolHandler factory, Promise.resolve wrapping, execute() without async)

provides:
  - createPlanRoadmapTool — 4-action ToolHandler wiring RoadmapOrchestrator to the agent surface
  - DianoiaOrchestrator.completeRoadmap() — FSM roadmap->phase-planning transition + planning:phase-complete event
  - DianoiaOrchestrator.advanceToExecution() — FSM phase-planning->executing transition + planning:phase-started event
  - DianoiaOrchestrator.listPhases() — thin delegator to store.listPhases()
  - DianoiaOrchestrator.getPhase() — thin delegator to store.getPhase()

affects:
  - 06-03 (wires plan_roadmap tool into organon registry alongside plan_requirements)
  - 09-verification (completeRoadmap/advanceToExecution are integration test targets)

tech-stack:
  added: []
  patterns:
    - ToolHandler factory pattern with Promise.resolve() wrapping (no async keyword on execute())
    - Conditional spread for exactOptionalPropertyTypes-compatible optional object args
    - Mock DianoiaOrchestrator via cast-through-unknown with vi.fn() stubs

key-files:
  created:
    - infrastructure/runtime/src/dianoia/roadmap-tool.ts
    - infrastructure/runtime/src/dianoia/roadmap-tool.test.ts
  modified:
    - infrastructure/runtime/src/dianoia/orchestrator.ts

key-decisions:
  - "plan_roadmap generate action commits roadmap on draft write-on-generate (both interactive and yolo) — survives restart; yolo also calls completeRoadmap immediately"
  - "plan_phases uses sequential reduce chain (not parallel) — PHAS-01 requires sequential phase planning"
  - "Mock DianoiaOrchestrator in roadmap-tool tests via cast-through-unknown with vi.fn() stubs (not real DianoiaOrchestrator) — avoids DB state coupling between test layers"
  - "completeRoadmap and advanceToExecution added to DianoiaOrchestrator (not RoadmapOrchestrator) — FSM transitions are orchestrator's domain"

requirements-completed: [ROAD-01, ROAD-02, ROAD-03, ROAD-04, ROAD-05, PHAS-01, PHAS-02, PHAS-03, PHAS-04, PHAS-05, PHAS-06]

duration: 3min
completed: 2026-02-24
---

# Phase 6 Plan 2: plan_roadmap Tool and Orchestrator Methods Summary

**plan_roadmap 4-action ToolHandler wiring RoadmapOrchestrator to the agent surface, plus completeRoadmap/advanceToExecution FSM transitions on DianoiaOrchestrator**

## Performance

- **Duration:** 3 min
- **Started:** 2026-02-24T16:04:26Z
- **Completed:** 2026-02-24T16:07:35Z
- **Tasks:** 3
- **Files modified:** 3

## Accomplishments
- Created `roadmap-tool.ts` with `createPlanRoadmapTool` factory handling generate (interactive/yolo), adjust_phase, commit, and plan_phases actions
- Created `roadmap-tool.test.ts` with 7 behavioral tests covering all 4 actions and edge cases (coverage failure, yolo auto-commit, coverage gate rejection)
- Added `completeRoadmap`, `advanceToExecution`, `listPhases`, `getPhase` to `DianoiaOrchestrator` following the `completeRequirements` pattern exactly

## Task Commits

Each task was committed atomically:

1. **Task 1: Create plan_roadmap tool (roadmap-tool.ts)** - `4d3384b` (feat)
2. **Task 2: Add behavioral tests for plan_roadmap tool (roadmap-tool.test.ts)** - `187bc37` (test)
3. **Task 3: Add orchestrator methods (completeRoadmap, advanceToExecution, listPhases, getPhase)** - `5263dbd` (feat)

## Files Created/Modified
- `infrastructure/runtime/src/dianoia/roadmap-tool.ts` - 4-action ToolHandler for the plan_roadmap agent tool
- `infrastructure/runtime/src/dianoia/roadmap-tool.test.ts` - 7 behavioral tests (generate interactive/yolo/coverage-fail, adjust_phase, commit pass/fail, plan_phases)
- `infrastructure/runtime/src/dianoia/orchestrator.ts` - Added completeRoadmap, advanceToExecution, listPhases, getPhase methods

## Decisions Made
- `plan_roadmap generate` calls `commitRoadmap` in both interactive and yolo modes (write-on-generate pattern). In yolo mode it additionally calls `completeRoadmap` immediately. In interactive mode it returns a draft display for the agent to show the user.
- `plan_phases` uses a sequential `.reduce()` promise chain rather than `Promise.all()` — the plan specifies sequential execution to avoid parallelism in phase planning (PHAS-01 requirement).
- Mock `DianoiaOrchestrator` in tests built via `cast-through-unknown` with `vi.fn()` stubs pointing at real `PlanningStore`. This avoids creating a real `DianoiaOrchestrator` instance (which needs default config) while still exercising real DB reads via `store.getProject()` and `store.listPhases()`.
- `completeRoadmap` and `advanceToExecution` live on `DianoiaOrchestrator` rather than `RoadmapOrchestrator` — FSM state transitions are the orchestrator's domain; `RoadmapOrchestrator` already has parallel `transitionToPhysicalPlanning`/`transitionToExecution` helpers but the tool uses the main orchestrator's versions for consistency with how `completeRequirements` works.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed exactOptionalPropertyTypes incompatibility in adjustPhase call**
- **Found during:** Task 1 (creating roadmap-tool.ts)
- **Issue:** Passing `{ phaseName, requirements, newName, newGoal }` directly failed `exactOptionalPropertyTypes` because `string | undefined` is not assignable to `string` in optional property position
- **Fix:** Applied conditional spread pattern: `...(phaseName !== undefined ? { phaseName } : {})` for all four optional opts fields
- **Files modified:** `infrastructure/runtime/src/dianoia/roadmap-tool.ts`
- **Verification:** `npx tsc --noEmit` clean
- **Committed in:** `4d3384b` (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 bug — exactOptionalPropertyTypes pattern)
**Impact on plan:** Fix was necessary for TypeScript correctness. Same pattern used in orchestrator.ts Task 03-01 and elsewhere in the codebase.

## Issues Encountered
- Task 3 was implemented before committing Task 1 because Task 1's TSC verification (`npx tsc --noEmit`) requires the orchestrator methods to exist (forward dependency). Both files were correct; execution order was adjusted to satisfy the type checker before committing Task 1.

## Next Phase Readiness
- `plan_roadmap` tool is ready to be registered in the organon tool registry (Phase 6 Plan 3)
- All 165 dianoia tests pass; no regressions
- FSM transitions for roadmap completion and execution advancement are wired and tested

---
*Phase: 06-roadmap-phase-planning*
*Completed: 2026-02-24*
