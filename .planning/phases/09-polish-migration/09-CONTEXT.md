# Phase 9: Polish & Migration - Context

**Gathered:** 2026-02-24
**Status:** Ready for planning

<domain>
## Phase Boundary

Get the Dianoia codebase PR-ready: write the spec document (`docs/specs/31_dianoia.md`), add an integration test that walks the full pipeline from idle to complete, update CONTRIBUTING.md with Dianoia module conventions, pass whole-codebase type-check and lint, and implement the status pill UI component deferred from Phase 7. Nothing new is designed — existing functionality is documented, validated, and polished.

</domain>

<decisions>
## Implementation Decisions

### Spec document format and depth
- **State machine diagram**: both Mermaid `stateDiagram-v2` (primary) and ASCII fallback in a comment block — renders on GitHub and in editors that don't support Mermaid
- **API surface section**: between endpoint-list-with-shapes and full OpenAPI — every route with HTTP method, path params, and request/response JSON shapes; include common error codes and status meanings, but no exhaustive OpenAPI schema objects
- **SQLite schema section**: full CREATE TABLE DDL for all 5+ tables (v20–v25 migrations) — exact DDL is the canonical reference
- **Problem section**: why existing session-scoped tools fall short + what Dianoia enables — brief product framing + technical gap statement, not user stories or competitive landscape
- **7 required sections**: Problem, Design, SQLite schema, state machine diagram, API surface, Implementation Order, Success Criteria

### Integration test
- **Scope**: happy path — create project, mock sub-agent dispatch, walk through pipeline phases (research → requirements → roadmap → execution → verification), reach `complete` state
- **Failure coverage**: Claude decides based on what gaps remain after unit tests; happy path is the minimum required
- **Location**: `infrastructure/runtime/src/dianoia/dianoia.integration.test.ts`
- **Mocking**: Claude decides the mocking strategy for `sessions_dispatch` (constructor-injected stub is the natural fit given existing patterns)

### CONTRIBUTING.md conventions
- **Depth**: module overview + key patterns + gotchas — enough for a contributor to understand Dianoia without reading the spec doc end-to-end
- **All 4 gotchas must be documented**:
  1. Migration propagation — when adding a migration, all dianoia test `makeDb()` helpers must be updated
  2. `exactOptionalPropertyTypes` — use conditional spread `...(x !== undefined ? { x } : {})` not direct assignment
  3. `oxlint require-await` — sync tool branches must use `return Promise.resolve()` not `async` keyword
  4. Orchestrator registration — new orchestrators go through `NousManager` setter/getter + conditional spread into `RouteDeps` in `server.ts`
- **Structure**: overview in CONTRIBUTING.md, deep design detail in spec doc — cross-link both ways

### Status pill UI (deferred from Phase 7)
- **Include in Phase 9** — Phase 7 CONTEXT.md deferred it here explicitly; Phase 9 is the Polish phase
- Phase 7 built the backend API (`GET /api/planning/projects/:id/execution`); Phase 9 adds the Svelte UI component
- Target: a status pill in the chat interface that shows current execution state; when clicked, opens a right pane with collapsible per-agent status (similar to the existing tool-use/thinking pills)
- Data source: execution API endpoint from Phase 7
- Claude decides the exact Svelte component structure and polling/SSE strategy

### Type-check / lint strategy
- **Scope**: whole codebase — `npx tsc --noEmit` and `npx oxlint src/` run against entire `infrastructure/runtime/src/`
- **Pre-existing issues**: fix them — the goal is a clean PR, not just clean Dianoia code
- **Non-trivial pre-existing issues**: Claude decides based on severity — trivial issues fixed, anything requiring significant rework gets documented in PR description instead

### Claude's Discretion
- Sessions_dispatch mock strategy in integration test
- Which failure paths (if any) to add to integration test beyond happy path
- Exact Svelte component structure for status pill and right-pane agent status
- Polling vs SSE for execution status updates in UI

</decisions>

<specifics>
## Specific Ideas

- Status pill should feel like the existing tool-use pills and thinking pills in the Aletheia UI — same visual language, collapsible right pane pattern
- The spec doc is a permanent artifact — it should be written as if someone is reading it cold to understand the module

</specifics>

<deferred>
## Deferred Ideas

- None — discussion stayed within phase scope (status pill was already scoped as deferred-to-Phase-9 in Phase 7 CONTEXT.md)

</deferred>

---

*Phase: 09-polish-migration*
*Context gathered: 2026-02-24*
