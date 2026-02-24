---
phase: 02-orchestrator-and-entry
plan: "03"
subsystem: planning
tags: [dianoia, intent-detection, context-injection, tdd, regex, vitest]

# Dependency graph
requires:
  - phase: 02-orchestrator-and-entry
    plan: "01"
    provides: "DianoiaOrchestrator.getActiveProject() and planningOrchestrator wiring in context.ts"
provides:
  - "detectPlanningIntent(text: string): boolean — pure function, two-signal detection"
  - "Planning offer injected into system prompt when no active project and intent detected"
  - "detectPlanningIntent exported from dianoia/index.ts public API"
affects: [phase-03-research, context-stage, nous-pipeline]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "TDD RED/GREEN cycle: failing import → implementation → wiring"
    - "Two-signal intent detection: action verb + project-scale noun, or complexity indicator + noun"
    - "Pure function with regex literals only — no imports, no state"

key-files:
  created:
    - infrastructure/runtime/src/dianoia/intent.ts
    - infrastructure/runtime/src/dianoia/intent.test.ts
  modified:
    - infrastructure/runtime/src/nous/pipeline/stages/context.ts
    - infrastructure/runtime/src/dianoia/index.ts

key-decisions:
  - "Two-signal detection required for build/create verbs: must co-occur with project-scale noun to avoid false positives on everyday messages"
  - "Explicit phrases (help me plan, new project, /plan) are single-signal sufficient — unambiguous enough without second signal"
  - "Intent offer only injected when planningOrchestrator is present AND no active project — avoids duplicate injection when project already active"
  - "Offer is else-branch of activeProject check, not separate if block — cleaner nesting, single pass"

patterns-established:
  - "Pure detection functions live in dianoia/intent.ts — no imports, regex literals only"
  - "Context stage injection: planning-related blocks colocated in one if(planningOrchestrator) block"

requirements-completed: [ENTRY-02, TEST-03]

# Metrics
duration: 2min
completed: 2026-02-24
---

# Phase 2 Plan 3: Intent Detection Hook Summary

**Pure detectPlanningIntent() function with two-signal regex detection, 17-test TDD coverage, and non-blocking planning offer injected into context stage when no active project**

## Performance

- **Duration:** ~2 min
- **Started:** 2026-02-24T00:10:48Z
- **Completed:** 2026-02-24T00:13:17Z
- **Tasks:** 3 (RED, GREEN, WIRE)
- **Files modified:** 4

## Accomplishments

- Implemented `detectPlanningIntent()` as a pure function with regex-only detection (no imports, no side effects)
- TDD: RED commit with 17 failing cases, GREEN commit with all passing
- Wired intent detection into context.ts: injects planning offer only when no active project and intent detected
- Exported `detectPlanningIntent` from dianoia/index.ts public API

## Task Commits

Each task was committed atomically:

1. **Task 1: RED — failing intent detection tests** - `f5765e4` (test)
2. **Task 2: GREEN — implement detectPlanningIntent** - `3fdbe31` (feat)
3. **Task 3: WIRE — context.ts injection + index.ts export** - `b812a90` (feat)

_TDD plan: test commit → implementation commit → wiring commit_

## Files Created/Modified

- `infrastructure/runtime/src/dianoia/intent.ts` - Pure function: action verb + project-scale noun, or complexity indicators, or explicit phrases
- `infrastructure/runtime/src/dianoia/intent.test.ts` - 7 true-positive + 10 false-positive test cases (17 total)
- `infrastructure/runtime/src/nous/pipeline/stages/context.ts` - Added intent detection offer in else-branch of active project check
- `infrastructure/runtime/src/dianoia/index.ts` - Added `export { detectPlanningIntent } from "./intent.js"`

## Decisions Made

- Two signals required for `build`/`create` verbs (project-scale noun must co-occur) to prevent "build a function" or "create a file" from triggering
- `help me plan`, `new project`, `/plan` treated as single-signal sufficient — semantically unambiguous
- Offer injected as `else` branch: `if (activeProject) { inject state } else if (detectPlanningIntent) { inject offer }` — clean mutual exclusion
- `detectPlanningIntent` exported from public API so downstream modules (e.g., future CLI) can reuse it without importing internals

## Deviations from Plan

None — plan executed exactly as written. Plan specified the exact structure; implementation matched the specification.

The `vitest run` flag `-x` (stop on first failure) was not supported by this vitest version; removed the flag. No behavioral change.

## Issues Encountered

None.

## Next Phase Readiness

- ENTRY-02 and TEST-03 complete: intent detection with well-defined boundaries is in production
- Phase 2 plan sequence complete (02-01 orchestrator, 02-02 /plan command, 02-03 intent detection)
- Phase 3 (Research) can proceed — depends on Phase 2 orchestrator infrastructure only

## Self-Check: PASSED

- intent.ts: FOUND
- intent.test.ts: FOUND
- 02-03-SUMMARY.md: FOUND
- Commit f5765e4 (RED): FOUND
- Commit 3fdbe31 (GREEN): FOUND
- Commit b812a90 (WIRE): FOUND

---
*Phase: 02-orchestrator-and-entry*
*Completed: 2026-02-24*
