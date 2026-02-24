---
phase: 03-project-context-and-api
plan: "02"
subsystem: api
tags: [hono, planning, http-routes, pylon]

requires:
  - phase: 03-01
    provides: DianoiaOrchestrator with project context persistence and questioning loop
  - phase: 02-01
    provides: DianoiaOrchestrator class and NousManager.setPlanningOrchestrator/getPlanningOrchestrator

provides:
  - GET /api/planning/projects — returns JSON array of all planning projects (summary fields)
  - GET /api/planning/projects/:id — returns full project snapshot including projectContext, or 404
  - RouteDeps extended with planningOrchestrator?: DianoiaOrchestrator (non-breaking optional field)
  - planningRoutes Hono factory wired into createGateway modules array

affects:
  - phase 03-03 (additional planning API endpoints will follow this same factory pattern)
  - phase 09 (E2E tests will exercise these routes)

tech-stack:
  added: []
  patterns:
    - "planningRoutes(deps, refs): Hono factory pattern — same shape as all other pylon route modules"
    - "Optional RouteDeps field + conditional spread for exactOptionalPropertyTypes compatibility"

key-files:
  created:
    - infrastructure/runtime/src/dianoia/routes.ts
  modified:
    - infrastructure/runtime/src/pylon/routes/deps.ts
    - infrastructure/runtime/src/pylon/server.ts
    - infrastructure/runtime/src/dianoia/orchestrator.ts

key-decisions:
  - "exactOptionalPropertyTypes requires conditional spread (...(val ? { planningOrchestrator: val } : {})) — direct assignment of DianoiaOrchestrator | undefined fails type check"
  - "listAllProjects() and getProject() added as thin public accessors on DianoiaOrchestrator delegating to PlanningStore — routes never reach through to store directly"
  - "GET /api/planning/projects returns summary fields only (id, goal, state, createdAt, updatedAt) — full snapshot only on /:id per CONTEXT.md no-tiered-responses decision"

patterns-established:
  - "New route modules in non-pylon modules (e.g. dianoia) import RouteDeps/RouteRefs from ../pylon/routes/deps.js"
  - "503 returned when optional orchestrator dep is absent — consistent service-unavailable pattern for optional subsystems"

requirements-completed: [INTG-01, INTG-02]

duration: 2min
completed: 2026-02-24
---

# Phase 3 Plan 02: Planning HTTP API Routes Summary

**Hono factory `planningRoutes` exposing GET /api/planning/projects and GET /api/planning/projects/:id, wired into pylon's createGateway with conditional-spread exactOptionalPropertyTypes fix**

## Performance

- **Duration:** 2 min
- **Started:** 2026-02-24T00:47:03Z
- **Completed:** 2026-02-24T00:48:35Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- Extended `RouteDeps` with optional `planningOrchestrator?: DianoiaOrchestrator` — no breaking change to any existing route module
- Created `dianoia/routes.ts` with the standard Hono factory pattern: 503 when orchestrator absent, 404 when project not found, full snapshot on /:id
- Added `listAllProjects()` and `getProject()` public accessors to `DianoiaOrchestrator` (thin delegation to `PlanningStore`)
- Wired `planningRoutes` into `createGateway()` modules array with `exactOptionalPropertyTypes`-safe conditional spread

## Task Commits

1. **Task 1: Extend RouteDeps and create planningRoutes factory** - `d1cbb08` (feat)
2. **Task 2: Wire planningRoutes into createGateway** - `89e664f` (feat)

## Files Created/Modified

- `infrastructure/runtime/src/dianoia/routes.ts` - planningRoutes Hono factory with two GET handlers
- `infrastructure/runtime/src/pylon/routes/deps.ts` - Added DianoiaOrchestrator import and planningOrchestrator? field to RouteDeps
- `infrastructure/runtime/src/pylon/server.ts` - Import planningRoutes, conditional spread for deps, added to modules array
- `infrastructure/runtime/src/dianoia/orchestrator.ts` - Added listAllProjects() and getProject() public accessors

## Decisions Made

- `exactOptionalPropertyTypes` (tsconfig strict flag) prevented direct `planningOrchestrator: manager.getPlanningOrchestrator()` assignment — coerced to conditional spread `...(val ? { planningOrchestrator: val } : {})`. Same pattern used in 03-01 for optional array fields.
- `listAllProjects()` and `getProject()` added as thin accessors on `DianoiaOrchestrator` rather than exposing `store` publicly — keeps the store private, routes work through the orchestrator boundary.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] exactOptionalPropertyTypes rejected direct undefined assignment**
- **Found during:** Task 2 (Wire planningRoutes into createGateway)
- **Issue:** `planningOrchestrator: manager.getPlanningOrchestrator()` fails type check — `DianoiaOrchestrator | undefined` not assignable to `DianoiaOrchestrator` under `exactOptionalPropertyTypes: true`
- **Fix:** Extracted to local variable, used conditional spread `...(planningOrchestrator ? { planningOrchestrator } : {})`
- **Files modified:** infrastructure/runtime/src/pylon/server.ts
- **Verification:** `npx tsc --noEmit` passes with zero errors
- **Committed in:** `89e664f` (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (Rule 1 — type error from strict tsconfig)
**Impact on plan:** Necessary for type correctness under existing tsconfig. Pattern is consistent with 03-01 precedent.

## Issues Encountered

None beyond the auto-fixed type error above.

## Next Phase Readiness

- Planning HTTP API is live — GET /api/planning/projects and GET /api/planning/projects/:id respond when server is running
- Phase 03-03 (additional planning API endpoints) can follow the identical Hono factory pattern in `dianoia/routes.ts`
- Pre-existing server.ts oxlint warnings (3x `require-await` on middleware closures) are out of scope, logged to deferred items

---
*Phase: 03-project-context-and-api*
*Completed: 2026-02-24*
