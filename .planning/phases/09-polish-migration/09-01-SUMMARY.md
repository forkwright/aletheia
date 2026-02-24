---
phase: 09-polish-migration
plan: "01"
subsystem: docs
tags: [dianoia, planning, spec, documentation, state-machine, sqlite, api]

requires:
  - phase: 08-verification-checkpoints
    provides: GoalBackwardVerifier, CheckpointSystem, plan_verify tool, v25 migration, DianoiaOrchestrator FSM methods

provides:
  - "Canonical Dianoia spec document at docs/specs/31_dianoia.md"
  - "Complete DDL reference for all 6 planning tables across v20-v25 migrations"
  - "Mermaid stateDiagram-v2 with all 11 states and ASCII fallback"
  - "API surface documentation for all 5 planning routes with TypeScript interfaces"
  - "Implementation Order tracing all 8 build phases"

affects:
  - 09-02-integration-test
  - 09-03-contributing-lint
  - 09-04-status-pill-ui

tech-stack:
  added: []
  patterns: []

key-files:
  created:
    - docs/specs/31_dianoia.md
  modified: []

key-decisions:
  - "Spec follows metadata-table format (not frontmatter) matching specs 29 and 30"
  - "DDL sourced verbatim from schema.ts constants to ensure accuracy"
  - "State machine diagram includes both Mermaid stateDiagram-v2 and ASCII fallback in HTML comment"
  - "API surface documents TypeScript interfaces inline alongside JSON examples"

patterns-established:
  - "Spec document format: metadata table, 7 sections, Mermaid + ASCII fallback"

requirements-completed:
  - DOCS-01
  - DOCS-02

duration: 2min
completed: 2026-02-24
---

# Phase 9 Plan 01: Dianoia Spec Document Summary

**597-line canonical spec for the Dianoia planning runtime: 7 sections, full v20-v25 DDL, Mermaid state machine diagram, and all 5 API routes with TypeScript interfaces**

## Performance

- **Duration:** 2 min
- **Started:** 2026-02-24T19:25:13Z
- **Completed:** 2026-02-24T19:27:26Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments

- Wrote `docs/specs/31_dianoia.md` with all 7 required sections (Problem, Design, SQLite Schema, State Machine, API Surface, Implementation Order, Success Criteria)
- Documented complete CREATE TABLE DDL for all 5 base tables (v20) plus planning_spawn_records (v24) and all 5 ALTER TABLE migrations (v21-v25)
- Included full Mermaid `stateDiagram-v2` block with all 11 states and 15 transitions, plus ASCII fallback in HTML comment
- Documented all 5 API routes with HTTP method, path parameters, JSON response shapes, and TypeScript interfaces (ExecutionSnapshot, PlanEntry, PhasePlanStatus, PlanningProject, ProjectContext)
- Traced 8 implementation phases in the Implementation Order section, each with a one-sentence description of what it delivered

## Task Commits

Each task was committed atomically:

1. **Task 1: Write docs/specs/31_dianoia.md** - `b6e9760` (feat)

**Plan metadata:** (committed with SUMMARY.md in docs commit)

## Files Created/Modified

- `docs/specs/31_dianoia.md` - Canonical Dianoia design document (597 lines)

## Decisions Made

- Followed metadata-table format matching specs 29 and 30 rather than frontmatter template style
- DDL sourced verbatim from `schema.ts` constants — accurate by construction, no risk of drift
- API surface documented with inline TypeScript interfaces (not separate section) so response shapes and types are co-located
- State semantics table added below the diagram to explain what each state means at runtime

## Deviations from Plan

None — plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None — no external service configuration required.

## Next Phase Readiness

- `docs/specs/31_dianoia.md` is the anchor reference for the integration test (09-02) and CONTRIBUTING.md additions (09-03)
- Spec covers the complete module surface; 09-02 and 09-03 can proceed independently

## Self-Check: PASSED

- FOUND: docs/specs/31_dianoia.md (597 lines, 7 ## sections)
- FOUND: commit b6e9760 (feat(09-01): write docs/specs/31_dianoia.md)

---
*Phase: 09-polish-migration*
*Completed: 2026-02-24*
