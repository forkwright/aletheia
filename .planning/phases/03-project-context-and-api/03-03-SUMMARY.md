---
phase: 03-project-context-and-api
plan: "03"
subsystem: planning
tags: [dianoia, context-injection, system-prompt, questioning-loop]

# Dependency graph
requires:
  - phase: 03-01
    provides: confirmSynthesis() persisting coreValue/constraints/keyDecisions to projectContext in DB
  - phase: 02-01
    provides: DianoiaOrchestrator with getActiveProject(), hasPendingConfirmation(), getNextQuestion()
provides:
  - Enriched planning context block in system prompt showing synthesized projectContext fields
  - Planning Question injection driving the conversational questioning loop via getNextQuestion()
  - Distillation-safe context delivery (re-read from DB each turn, not from conversation history)
affects: [04-research-phase, 05-requirements, 06-roadmap, 07-phase-planning, 08-execution, 09-verification]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "lines[] array construction for multi-field system prompt blocks with conditional fields"
    - "DB-fresh re-read pattern: context block rebuilt from store every turn — survives distillation automatically"
    - "State-conditional injection: getNextQuestion() only called when state === 'questioning'"

key-files:
  created: []
  modified:
    - infrastructure/runtime/src/nous/pipeline/stages/context.ts

key-decisions:
  - "getNextQuestion(projectId) called with activeProject.id (not nousId) — matches orchestrator signature"
  - "nextQuestion guard uses state === 'questioning' before calling getNextQuestion — avoids unnecessary DB reads"
  - "Planning Question injected as separate H2 section below project fields for clear LLM salience"
  - "No changes to distillation pipeline needed — injection is already DB-fresh per turn"

patterns-established:
  - "Conditional system prompt fields: push only when value is truthy/non-empty — keeps block minimal when context sparse"
  - "lines.join('\\n') over template literal for multi-line blocks with optional sections"

requirements-completed: [PROJ-04, INTG-03, INTG-04]

# Metrics
duration: 1min
completed: 2026-02-24
---

# Phase 3 Plan 03: Enriched Planning Context Block Summary

**System prompt planning block enriched with synthesized projectContext fields (coreValue, constraints, keyDecisions) and next-question injection driving the questioning loop via getNextQuestion()**

## Performance

- **Duration:** 1 min
- **Started:** 2026-02-24T00:30:15Z
- **Completed:** 2026-02-24T00:30:50Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments

- Planning context block now conditionally renders coreValue, constraints, and keyDecisions from synthesized projectContext
- Next planning question injected as `## Planning Question` section when project state is `questioning`, driving conversational loop
- Distillation safety satisfied: context block is re-read from DB on every turn, not from conversation history
- All 5 planning events confirmed reachable via grep: project-created (x2), project-resumed, phase-started, phase-complete, complete — INTG-03 verified

## Task Commits

Each task was committed atomically:

1. **Task 1: Enrich planning context block and inject next question** - `2ca94a6` (feat)

**Plan metadata:** `e98e980` (docs: complete enriched planning context block plan)

## Files Created/Modified

- `infrastructure/runtime/src/nous/pipeline/stages/context.ts` — Replaced single template literal with lines[] array; added conditional coreValue, constraints, keyDecisions, and Planning Question section

## Decisions Made

- `getNextQuestion()` called with `activeProject.id` not `nousId` — the orchestrator method signature takes a project ID
- `nextQuestion` only computed when `state === 'questioning'` — avoids unnecessary DB reads for other states
- Planning Question rendered as `## Planning Question` H2 sub-section for clear LLM salience, separated from project metadata by blank line
- No distillation pipeline changes needed — the injection already re-reads from store on each turn (PROJ-04 satisfied structurally)

## Deviations from Plan

None — plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None — no external service configuration required.

## Next Phase Readiness

- Phase 3 complete: planning context block fully enriched, questioning loop driven by context injection
- Phase 4 (Research) depends only on Phase 2 and can execute; planning orchestrator now delivers full context to every turn
- No blockers

---
*Phase: 03-project-context-and-api*
*Completed: 2026-02-24*
