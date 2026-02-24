---
phase: 06-roadmap-phase-planning
plan: "01"
subsystem: planning
tags: [dianoia, roadmap, orchestrator, tdd, vitest, sqlite, better-sqlite3]

requires:
  - phase: 05-requirements-definition
    provides: PlanningStore.listRequirements(), PlanningStore.createPhase(), planning_phases table with plan/status columns

provides:
  - RoadmapOrchestrator class (roadmap.ts) — generates roadmap, commits phases, validates coverage, plans phases with checker loop
  - PhaseDefinition, PhasePlan, PlanStep interfaces (exported from roadmap.ts)
  - depthToInstruction() — calibrates plan depth for quick/standard/comprehensive
  - formatRoadmapDisplay() — markdown rendering of phases for user review
  - commitRoadmap() — atomic db.transaction() batch insert/replace of phases
  - validateCoverage() / validateCoverageFromDb() — v1 requirement coverage gate
  - planPhase() — dispatches planner + checker loop up to MAX_ITERATIONS=3

affects:
  - 06-02 (plan_roadmap tool depends on all RoadmapOrchestrator methods)
  - 06-03 (API route wiring depends on RoadmapOrchestrator constructor signature)

tech-stack:
  added: []
  patterns:
    - "RoadmapOrchestrator(db, dispatchTool) constructor — same as ResearchOrchestrator and RequirementsOrchestrator; store db as instance field for transaction wrapper"
    - "commitRoadmap uses db.transaction() for atomic DELETE + createPhase() batch — avoids partial-roadmap corruption"
    - "planPhase checker loop: generate → check → revise → check (repeat up to MAX_ITERATIONS=3); store best-effort plan on exhaustion"
    - "checkPlan dispatch failure returns {pass: true} — best-effort fallback prevents checker from blocking progress"
    - "parsePlanFromDispatch extracts JSON from ```json block using same regex as researcher.ts"

key-files:
  created:
    - infrastructure/runtime/src/dianoia/roadmap.ts
    - infrastructure/runtime/src/dianoia/roadmap.test.ts
  modified: []

key-decisions:
  - "depthToInstruction is a public method on RoadmapOrchestrator — tested directly, no need for separate export"
  - "adjustPhase builds dynamic SET clause (same pattern as store.updateRequirement) — supports name/goal/requirements independently"
  - "commitRoadmap stores db as instance field (this.db) alongside this.store — needed for transaction wrapper that can't go through store indirection"
  - "checkPlan dispatch failure is best-effort pass, not an error — prevents checker from blocking plan generation when dispatch is unreliable"
  - "planPhase reads project config for depth via store.getProjectOrThrow — orchestrator owns depth selection, not callers"

patterns-established:
  - "Checker loop pattern: generate → [check → revise]^N → store (best-effort on exhaustion) — matches GSD plan_check pattern"
  - "Agent dispatch result parsing: JSON.parse(raw) as DispatchOutput → results[0] — same pattern as ResearchOrchestrator"

requirements-completed: [ROAD-01, ROAD-02, ROAD-03, ROAD-04, ROAD-05, PHAS-02, PHAS-03, PHAS-05, PHAS-06]

duration: 10min
completed: 2026-02-24
---

# Phase 6 Plan 01: RoadmapOrchestrator Summary

**RoadmapOrchestrator with 20 unit tests — roadmap generation, atomic phase commit, coverage validation, adjustPhase, and 3-iteration plan checker loop with best-effort fallback**

## Performance

- **Duration:** 10 min
- **Started:** 2026-02-24T15:51:09Z
- **Completed:** 2026-02-24T16:02:00Z
- **Tasks:** 3 (RED, GREEN, REFACTOR)
- **Files modified:** 2

## Accomplishments

- Full TDD cycle: 20 failing tests → implementation → all pass with 0 lint warnings
- RoadmapOrchestrator implements all 9 public methods specified in plan behavior
- commitRoadmap uses db.transaction() for atomic DELETE + INSERT — no partial-roadmap corruption possible
- planPhase checker loop matches MAX_ITERATIONS=3 GSD pattern with best-effort store on exhaustion
- adjustPhase uses dynamic SET construction identical to store.updateRequirement pattern

## Task Commits

1. **RED — failing tests** - `c472a5d` (test — on feat/aletheia-cli branch, superseded)
2. **GREEN — implement RoadmapOrchestrator** - `01c9908` (feat)
3. **REFACTOR — test file on main with lint fixes** - `63bbb95` (test)

## Files Created/Modified

- `infrastructure/runtime/src/dianoia/roadmap.ts` — RoadmapOrchestrator class + PhaseDefinition, PhasePlan, PlanStep interfaces
- `infrastructure/runtime/src/dianoia/roadmap.test.ts` — 20 unit tests covering all specified behaviors

## Decisions Made

- `depthToInstruction` exposed as public method — makes it directly testable without indirect dispatch
- `adjustPhase` logs the raw `adjustment` string for diagnostics but drives DB update purely from structured opts
- `checkPlan` dispatch failure returns `{pass: true, issues: []}` — best-effort avoids checker blocking plan generation when dispatch is unreliable
- `commitRoadmap` stores `this.db` as instance field (not just `this.store`) — required for `db.transaction()` wrapper that spans multiple `store.createPhase()` calls
- `planPhase` reads depth from `store.getProjectOrThrow(projectId).config.depth` — orchestrator owns depth selection, callers don't need to pass it

## Deviations from Plan

None — plan executed exactly as written. The branch mismatch for the initial RED commit was a tooling artifact (commit went to feat/aletheia-cli); resolved by creating test file fresh on main with lint fixes incorporated.

## Issues Encountered

- Initial RED commit landed on `feat/aletheia-cli` branch instead of `main` due to shell environment state; resolved by re-creating the test file directly on main with all lint fixes pre-applied

## Next Phase Readiness

- RoadmapOrchestrator is complete and test-covered; Plans 02 and 03 can proceed to implement the `plan_roadmap` tool and API routes
- All public methods used by the tool layer are defined — Plan 02 does not need to add any methods to this class

---
*Phase: 06-roadmap-phase-planning*
*Completed: 2026-02-24*
