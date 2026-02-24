---
phase: 03-project-context-and-api
plan: "04"
subsystem: api
tags: [organon, tools, deprecation, plan-propose, plan-create, dianoia]

# Dependency graph
requires:
  - phase: 02-orchestrator-and-entry
    provides: Dianoia orchestrator and /plan command that replaces plan_propose/plan_create
provides:
  - plan_propose and plan_create marked deprecated with JSDoc, description prefix, and deprecationWarning JSON key
affects: [future-agents, llm-tool-selection, backward-compatibility]

# Tech tracking
tech-stack:
  added: []
  patterns: [JSDoc @deprecated on function and variable declarations, deprecationWarning key in JSON tool returns]

key-files:
  created: []
  modified:
    - infrastructure/runtime/src/organon/built-in/plan-propose.ts
    - infrastructure/runtime/src/organon/built-in/plan.ts

key-decisions:
  - "deprecationWarning placed as JSON key inside JSON.stringify payload — never prepended as text — preserves PLAN_PROPOSED_MARKER JSON.parse compatibility"
  - "plan_status, plan_step_complete, plan_step_fail left unchanged — Phase 9 deferred per CONTEXT.md"

patterns-established:
  - "Tool deprecation pattern: JSDoc @deprecated + description prefix DEPRECATED: + deprecationWarning key in JSON return"

requirements-completed: [INTG-05]

# Metrics
duration: 1min
completed: 2026-02-24
---

# Phase 3 Plan 04: Deprecate plan_propose and plan_create Summary

**plan_propose and plan_create marked deprecated via JSDoc, description prefix, and in-payload deprecationWarning JSON key — PLAN_PROPOSED_MARKER parsing intact**

## Performance

- **Duration:** 1 min
- **Started:** 2026-02-24T00:52:30Z
- **Completed:** 2026-02-24T00:53:53Z
- **Tasks:** 1
- **Files modified:** 2

## Accomplishments
- `createPlanProposeHandler` receives JSDoc `@deprecated` and description prefixed with "DEPRECATED: Use /plan command instead."
- `deprecationWarning` key added inside `JSON.stringify({})` payload between `__marker` and `plan` keys — `PLAN_PROPOSED_MARKER` JSON parsing unaffected
- `planCreate` receives JSDoc `@deprecated` and description prefixed with "DEPRECATED: Use /plan command instead."
- `deprecationWarning` key added to `planCreate` JSON return alongside `planId`, `stepCount`, `actionableNow`
- `plan_status`, `plan_step_complete`, `plan_step_fail` left untouched as specified

## Task Commits

Each task was committed atomically:

1. **Task 1: Deprecate plan_propose and plan_create** - `da67c17` (feat)

**Plan metadata:** _(docs commit follows)_

## Files Created/Modified
- `infrastructure/runtime/src/organon/built-in/plan-propose.ts` - JSDoc @deprecated, DEPRECATED description prefix, deprecationWarning inside JSON
- `infrastructure/runtime/src/organon/built-in/plan.ts` - JSDoc @deprecated on planCreate, DEPRECATED description prefix, deprecationWarning in JSON return

## Decisions Made
- deprecationWarning is a JSON key inside the JSON.stringify() object, not prepended text. The `execute.ts` pipeline stage calls `JSON.parse()` on the plan_propose output to detect `PLAN_PROPOSED_MARKER`. Prepending any text would break that parse. The key stays inside the object.
- plan_status, plan_step_complete, plan_step_fail deferred to Phase 9 per CONTEXT.md decisions — only plan_create deprecated in this phase.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None. Pre-existing `require-await` oxlint warnings on all `async execute()` methods (zero await expressions) are out-of-scope pre-existing warnings, not introduced by this change.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- INTG-05 satisfied: both plan_propose and plan_create are deprecated with visible signals to the LLM
- LLMs calling these tools will see "DEPRECATED: Use /plan command instead." at the start of the description and receive `deprecationWarning` in the tool output
- Phase 4 (Research) can proceed independently; these tools remain fully functional for backward compatibility

## Self-Check: PASSED

All files present and commit da67c17 verified.

---
*Phase: 03-project-context-and-api*
*Completed: 2026-02-24*
