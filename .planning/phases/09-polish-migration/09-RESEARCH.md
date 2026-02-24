# Phase 9: Polish & Migration - Research

**Researched:** 2026-02-24
**Domain:** Documentation, integration testing, UI component, codebase quality
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Spec document format and depth
- **State machine diagram**: both Mermaid `stateDiagram-v2` (primary) and ASCII fallback in a comment block
- **API surface section**: every route with HTTP method, path params, and request/response JSON shapes; include common error codes and status meanings; no exhaustive OpenAPI schema objects
- **SQLite schema section**: full CREATE TABLE DDL for all 5+ tables (v20-v25 migrations)
- **Problem section**: why existing session-scoped tools fall short + what Dianoia enables; brief product framing + technical gap statement, not user stories or competitive landscape
- **7 required sections**: Problem, Design, SQLite schema, state machine diagram, API surface, Implementation Order, Success Criteria

#### Integration test
- **Scope**: happy path — create project, mock sub-agent dispatch, walk through pipeline phases (research → requirements → roadmap → execution → verification), reach `complete` state
- **Failure coverage**: Claude decides based on what gaps remain after unit tests; happy path is the minimum required
- **Location**: `infrastructure/runtime/src/dianoia/dianoia.integration.test.ts`
- **Mocking**: Claude decides the mocking strategy for `sessions_dispatch` (constructor-injected stub is the natural fit given existing patterns)

#### CONTRIBUTING.md conventions
- **Depth**: module overview + key patterns + gotchas — enough for a contributor to understand Dianoia without reading the spec doc end-to-end
- **All 4 gotchas must be documented**:
  1. Migration propagation — when adding a migration, all dianoia test `makeDb()` helpers must be updated
  2. `exactOptionalPropertyTypes` — use conditional spread `...(x !== undefined ? { x } : {})` not direct assignment
  3. `oxlint require-await` — sync tool branches must use `return Promise.resolve()` not `async` keyword
  4. Orchestrator registration — new orchestrators go through `NousManager` setter/getter + conditional spread into `RouteDeps` in `server.ts`
- **Structure**: overview in CONTRIBUTING.md, deep design detail in spec doc — cross-link both ways

#### Status pill UI (deferred from Phase 7)
- Phase 7 built the backend API (`GET /api/planning/projects/:id/execution`); Phase 9 adds the Svelte UI component
- Target: a status pill in the chat interface that shows current execution state; when clicked, opens a right pane with collapsible per-agent status (similar to the existing tool-use/thinking pills)
- Data source: execution API endpoint from Phase 7

#### Type-check / lint strategy
- **Scope**: whole codebase — `npx tsc --noEmit` and `npx oxlint src/` run against entire `infrastructure/runtime/src/`
- **Pre-existing issues**: fix them — the goal is a clean PR
- **Non-trivial pre-existing issues**: Claude decides based on severity — trivial issues fixed, anything requiring significant rework gets documented in PR description instead

### Claude's Discretion
- Sessions_dispatch mock strategy in integration test
- Which failure paths (if any) to add to integration test beyond happy path
- Exact Svelte component structure for status pill and right-pane agent status
- Polling vs SSE for execution status updates in UI

### Deferred Ideas (OUT OF SCOPE)
- None — discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DOCS-01 | Spec document `docs/specs/31_dianoia.md` follows established Aletheia spec format (Problem, Design, Implementation Order, Success Criteria) | Spec format confirmed from `_template.md` and spec 29/30 examples; spec number 31 is next in sequence |
| DOCS-02 | Spec includes SQLite schema definitions, state machine diagram, and API surface | Full DDL in schema.ts (v20-v25), all 11 FSM states confirmed in machine.ts, all 5 routes confirmed in routes.ts |
| DOCS-03 | CONTRIBUTING.md updated to note Dianoia module conventions | CONTRIBUTING.md reviewed; new `## Dianoia Module` section appended after existing `## Code Standards` section |
| TEST-04 | Integration test for the full planning pipeline (mock sessions_dispatch) | Existing unit tests show `makeDb()` pattern; constructor-injected `dispatchTool: ToolHandler` is the mock seam in `ExecutionOrchestrator`, `GoalBackwardVerifier`, `ResearchOrchestrator` |
| TEST-05 | `npx tsc --noEmit` passes with zero new type errors | tsc currently clean (0 output lines); test files excluded from tsc; no new errors to introduce |
| TEST-06 | `npx oxlint src/` passes with zero new lint errors | Currently 2 errors (both in dianoia test files); 144 warnings (all pre-existing in other modules) |
</phase_requirements>

---

## Summary

Phase 9 is a polish pass, not new feature work. All Dianoia functionality is complete across 22 plans in 8 prior phases (194 tests green). The work is: write the canonical spec document, add one integration test that validates the end-to-end pipeline, update CONTRIBUTING.md with four specific gotchas, add the Svelte status pill UI component (deferred from Phase 7), and pass whole-codebase typecheck and lint.

The codebase is in good shape entering this phase. `npx tsc --noEmit` returns zero output (clean). `npx oxlint src/` shows 2 errors and 144 warnings — all pre-existing, none in production code. The 2 errors are in `dianoia/checkpoint.test.ts` (missing `import type`) and `dianoia/execution.test.ts` (duplicate `./types.js` imports). Both are trivial one-line fixes. The 144 warnings are spread across other modules (`require-await` in test mocks, etc.) and the CONTEXT.md decision is to fix trivial issues and document non-trivial ones.

The status pill is the only genuine new UI code. The existing `ToolStatusLine.svelte` and `ThinkingPanel.svelte` provide exact patterns: a `<button class="...-status-line">` pill with Spinner + status text + chevron, and a `ThinkingPanel.svelte`-style right pane (380px, slide-in animation, `border-left: 1px solid var(--border)`). The execution API at `GET /api/planning/projects/:id/execution` returns an `ExecutionSnapshot` with `state`, `activeWave`, `plans[]`, and `activePlanIds`. Polling (setInterval) is the correct data strategy because the pylon server uses SSE for turn events but has no push mechanism for planning state changes — polling every 2-3 seconds while the pill is open is the correct approach.

**Primary recommendation:** Build in five logical tasks: (1) spec doc, (2) integration test, (3) CONTRIBUTING.md update, (4) status pill UI, (5) lint fixes. The spec doc is the longest artifact but its content is entirely derived from reading existing source files — no design decisions remain.

---

## Standard Stack

All tools for this phase are already in the project. No new dependencies.

### Core (already installed)
| Tool | Version | Purpose |
|------|---------|---------|
| vitest | (project version) | Integration test runner — same as all other dianoia tests |
| better-sqlite3 | (project version) | In-memory DB for integration test |
| Svelte 5 | (project version) | Status pill component — same as all other UI components |
| TypeScript | (project version) | Whole-codebase typecheck |
| oxlint | (project version) | Whole-codebase lint |

**Installation:** None required.

---

## Architecture Patterns

### Spec Document Format

Confirmed from `docs/specs/_template.md`, `docs/specs/29_ui-layout-and-theming.md`, and `docs/specs/30_homepage-dashboard.md`:

```
# Spec 31 — Dianoia: Persistent Multi-Phase Planning

| Field       | Value                     |
|-------------|---------------------------|
| Status      | Implemented               |
| Author      | Demiurge                  |
| Created     | 2026-02-24                |
| Scope       | infrastructure/runtime/src/dianoia/ |
| Spec        | 31                        |

---

## Problem
## Design
### Principles
### Architecture (state diagram here)
## SQLite Schema
## State Machine
## API Surface
## Implementation Order
## Success Criteria
```

**Key format observations from reading existing specs:**
- Specs 29 and 30 use a metadata table at the top (not the `_template.md` `**Author:**` frontmatter style) — the table format is current practice
- Sections use `##` level, subsections use `###`
- Code blocks are used liberally — DDL, TypeScript interfaces, ASCII diagrams
- Spec 30 uses ASCII diagram inline in a fenced code block
- Spec 29 uses tables for component-change summaries

### Mermaid State Diagram (all 11 states confirmed from machine.ts)

```mermaid
stateDiagram-v2
    [*] --> idle
    idle --> questioning : START_QUESTIONING
    idle --> abandoned : ABANDON
    questioning --> researching : START_RESEARCH
    questioning --> abandoned : ABANDON
    researching --> requirements : RESEARCH_COMPLETE
    researching --> blocked : BLOCK
    researching --> abandoned : ABANDON
    requirements --> roadmap : REQUIREMENTS_COMPLETE
    requirements --> abandoned : ABANDON
    roadmap --> phase-planning : ROADMAP_COMPLETE
    roadmap --> abandoned : ABANDON
    phase-planning --> executing : PLAN_READY
    phase-planning --> abandoned : ABANDON
    executing --> verifying : VERIFY
    executing --> blocked : BLOCK
    executing --> abandoned : ABANDON
    verifying --> phase-planning : NEXT_PHASE
    verifying --> complete : ALL_PHASES_COMPLETE
    verifying --> blocked : PHASE_FAILED
    verifying --> abandoned : ABANDON
    blocked --> executing : RESUME
    blocked --> abandoned : ABANDON
    complete --> [*]
    abandoned --> [*]
```

**ASCII fallback (for editors without Mermaid):**
```
idle --START_QUESTIONING--> questioning --START_RESEARCH--> researching
                                                         --RESEARCH_COMPLETE--> requirements
                                                         --BLOCK--> blocked
requirements --REQUIREMENTS_COMPLETE--> roadmap --ROADMAP_COMPLETE--> phase-planning
phase-planning --PLAN_READY--> executing --VERIFY--> verifying
                                         --BLOCK--> blocked
verifying --NEXT_PHASE--> phase-planning
          --ALL_PHASES_COMPLETE--> complete
          --PHASE_FAILED--> blocked
blocked --RESUME--> executing
[any active state] --ABANDON--> abandoned
```

### Integration Test Pattern (constructor-injected mock)

The natural mock seam: `ExecutionOrchestrator`, `GoalBackwardVerifier`, and `ResearchOrchestrator` all take `dispatchTool: ToolHandler` as a constructor argument. The `DianoiaOrchestrator` itself does not use `dispatchTool` — it is the state machine driver. The integration test wires them together via the same `makeDb()` helper established in all existing unit tests:

```typescript
// Source: pattern from orchestrator.test.ts + execution.test.ts
import Database from "better-sqlite3";
import { vi, describe, it, expect } from "vitest";
import {
  PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION,
  PLANNING_V23_MIGRATION, PLANNING_V24_MIGRATION, PLANNING_V25_MIGRATION,
} from "./schema.js";

function makeDb(): Database.Database {
  const db = new Database(":memory:");
  db.pragma("journal_mode = WAL");
  db.pragma("foreign_keys = ON");
  db.exec(PLANNING_V20_DDL);
  db.exec(PLANNING_V21_MIGRATION);
  db.exec(PLANNING_V22_MIGRATION);
  db.exec(PLANNING_V23_MIGRATION);
  db.exec(PLANNING_V24_MIGRATION);
  db.exec(PLANNING_V25_MIGRATION);
  return db;
}

// Mock dispatchTool — returns success for all dispatches
const mockDispatchTool = {
  definition: { name: "sessions_dispatch", description: "", input_schema: { type: "object", properties: {} } },
  execute: vi.fn().mockResolvedValue(
    JSON.stringify({ results: [{ status: "success", result: JSON.stringify({ status: "met", summary: "ok", gaps: [] }), durationMs: 100 }] })
  ),
};
```

The happy-path integration test must drive the full pipeline manually via orchestrator methods, since each pipeline phase is triggered by a tool call (plan_research, plan_requirements, plan_roadmap, plan_phases, plan_execute, plan_verify) — the integration test calls the orchestrator methods directly and verifies state transitions.

**Full pipeline path (idle → complete):**
1. `DianoiaOrchestrator.handle()` → state: `questioning`
2. `DianoiaOrchestrator.confirmSynthesis()` → state: `researching`
3. `DianoiaOrchestrator.skipResearch()` → state: `requirements` (or use ResearchOrchestrator with mocked dispatch)
4. `DianoiaOrchestrator.completeRequirements()` → state: `roadmap`
5. `DianoiaOrchestrator.completeRoadmap()` → state: `phase-planning`
6. `DianoiaOrchestrator.advanceToExecution()` → state: `executing`
7. `ExecutionOrchestrator.executePhase()` (with mocked dispatch) → spawn records created, phases marked complete
8. `DianoiaOrchestrator.advanceToVerification()` → state: `verifying`
9. `GoalBackwardVerifier.verify()` (with mocked dispatch returning `{status:"met"}`)
10. `DianoiaOrchestrator.completeAllPhases()` → state: `complete`

### Status Pill UI Pattern

Confirmed from reading `ToolStatusLine.svelte`, `ThinkingStatusLine.svelte`, `ThinkingPanel.svelte`, and `ChatView.svelte`:

**Pill component structure** (`PlanningStatusLine.svelte`):
- Same CSS class convention as `tool-status-line` and `thinking-status-line`
- Uses `border-radius: var(--radius-pill)`, `display: inline-flex`, `gap: 6px`, `padding: 4px 10px`
- Active state: `border-left: 3px solid var(--status-active)` (same as thinking pill)
- `Spinner` component from `../shared/Spinner.svelte` for active state
- `onclick` prop triggers panel open

**Right-pane panel structure** (`PlanningPanel.svelte`):
- Same dimensions and animation as `ThinkingPanel.svelte`: `width: 380px`, `animation: slide-in 0.15s ease`
- Panel body lists per-plan status with expand/collapse — same pattern as `ToolPanel.svelte`
- `onClose` prop, same close button pattern

**ChatView wiring** (add alongside existing ToolPanel/ThinkingPanel):
```svelte
// State in ChatView.svelte
let selectedPlanningProjectId = $state<string | null>(null);

// Render in .chat-area div — alongside ToolPanel and ThinkingPanel
{#if selectedPlanningProjectId}
  <PlanningPanel projectId={selectedPlanningProjectId} onClose={() => selectedPlanningProjectId = null} />
{/if}
```

**Polling strategy** (not SSE): The pylon SSE endpoint emits `planning:phase-started`, `planning:phase-complete`, `planning:complete` events on `eventBus`, but these are server-side events, not push events the UI SSE stream receives. The UI SSE stream carries `turn:before`/`turn:after`/`tool:called` events only. Therefore the correct approach is polling `GET /api/planning/projects/:id/execution` every 2-3 seconds when the panel is open:

```typescript
// $effect in PlanningPanel.svelte
$effect(() => {
  if (!projectId) return;
  fetchSnapshot(); // immediate
  const iv = setInterval(fetchSnapshot, 2500);
  return () => clearInterval(iv);
});
```

**API response shape** (`ExecutionSnapshot` from execution.ts):
```typescript
interface ExecutionSnapshot {
  projectId: string;
  state: string;           // DianoiaState
  activeWave: number | null;
  plans: PlanEntry[];      // per-phase status
  activePlanIds: string[];
  startedAt: string | null;
  completedAt: string | null;
}

interface PlanEntry {
  phaseId: string;
  name: string;
  status: string;          // pending|running|done|failed|skipped|zombie
  waveNumber: number | null;
  startedAt: string | null;
  completedAt: string | null;
  error: string | null;
}
```

**Pill status text logic** (derive from snapshot):
- `state === "executing" && activeWave !== null` → `"Wave ${activeWave + 1} running"`
- `state === "verifying"` → `"Verifying phase"`
- `state === "complete"` → `"Planning complete"`
- `state === "blocked"` → `"Blocked"`
- `state === "phase-planning"` → `"Planning phases"`
- Other active states → capitalize the state name

### CONTRIBUTING.md Section Placement

The current `CONTRIBUTING.md` ends with `## Code Standards` (line 184) followed by `## Reporting Issues` and `## License`. The new `## Dianoia Module` section goes between `## Code Standards` and `## Reporting Issues` — keeping code content grouped before meta sections.

The 4 gotchas documented verbatim from CONTEXT.md decisions:

1. **Migration propagation**: Every `makeDb()` helper in `src/dianoia/*.test.ts` must include ALL migrations through the current version. When a new migration is added (V26, etc.), update: `store.test.ts`, `orchestrator.test.ts`, `researcher.test.ts`, `requirements.test.ts`, `roadmap.test.ts`, `roadmap-tool.test.ts`, `execution.test.ts`, `verifier.test.ts`, `checkpoint.test.ts`.

2. **exactOptionalPropertyTypes**: `tsconfig.json` enables `exactOptionalPropertyTypes`. When merging objects with optional fields, use conditional spread:
   ```typescript
   // Wrong:
   const merged = { ...base, optionalField: value ?? undefined };
   // Right:
   const merged = { ...base, ...(value !== undefined ? { optionalField: value } : {}) };
   ```

3. **oxlint require-await**: `ToolHandler.execute()` implementations that are synchronous in some branches must use `return Promise.resolve(result)` instead of `async` keyword. The `async` keyword on a function with no `await` triggers `eslint(require-await)`.

4. **Orchestrator registration**: New orchestrators follow the `NousManager` setter/getter pattern (see `planningOrchestrator` and `executionOrchestrator`). They are set in `createRuntime()`, retrieved in `server.ts` via `manager.get*()`, and spread into `RouteDeps` using conditional spread (required by `exactOptionalPropertyTypes`):
   ```typescript
   // server.ts pattern:
   const orchValue = manager.getMyOrchestrator();
   const deps: RouteDeps = {
     ...base,
     ...(orchValue !== undefined ? { myOrchestrator: orchValue } : {}),
   };
   ```

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead |
|---------|-------------|-------------|
| DB setup for integration test | New migration runner | Import all 6 migration constants from `./schema.js`, call `db.exec()` for each — same pattern as every existing unit test |
| SSE push for planning status | Custom event emitter bridge | Polling every 2.5s via `setInterval` in `$effect` — pylon has no planning-specific push; SSE stream only carries turn events |
| State machine for spec diagram | Re-derive from code | Read `machine.ts` directly — `VALID_TRANSITIONS` and `TRANSITION_RESULT` are the authoritative source; diagram is a rendering of those two constants |
| API shape documentation | New documentation tool | Read `routes.ts` directly and document the 5 routes with their exact response shapes |

---

## Common Pitfalls

### Pitfall 1: Missing verificationResult field in makePhase() test helper
**What goes wrong:** `PlanningPhase` interface has `verificationResult: VerificationResult | null` (required, added in V25 to `planning_phases`). The `makePhase()` helper in `execution.test.ts` does not include this field. This does NOT cause a `tsc --noEmit` error because test files are excluded from tsconfig (`"exclude": ["**/*.test.ts"]`). It only shows up at runtime.
**Why it happens:** Test files excluded from tsc compilation check.
**How to avoid:** When writing `dianoia.integration.test.ts`, include `verificationResult: null` in any `PlanningPhase` stub object. When fixing `execution.test.ts`'s `makePhase()`, add `verificationResult: null`.
**Warning signs:** Vitest type errors at runtime that tsc doesn't catch.

### Pitfall 2: index.ts missing V24 and V25 migration exports
**What goes wrong:** `dianoia/index.ts` exports `PLANNING_V20_DDL`, `V21`, `V22`, `V23` but NOT `V24` (spawn records table) or `V25` (risk_level/verification_result columns). External consumers importing from `dianoia` don't have access to the full migration sequence.
**Why it happens:** Each migration was added in its phase but the export wasn't updated.
**How to avoid:** The integration test imports directly from `./schema.js` (not from `dianoia/index.ts`) — same as all unit tests. The fix (add V24/V25 to index.ts exports) should be included in this phase's lint/polish task.
**Warning signs:** External code importing migrations from the public API gets incomplete DDL.

### Pitfall 3: oxlint 2 errors are blocking (not warnings)
**What goes wrong:** `npx oxlint src/` currently exits with "2 errors" — `consistent-type-imports` in `checkpoint.test.ts` and `no-duplicates` in `execution.test.ts`. These are `x` (error) level, not `!` (warning) level.
**Why it happens:** Pre-existing issues from Phase 8.
**How to avoid:** Fix both before running the lint gate. Both are trivial:
- `checkpoint.test.ts` line 12: change `import { PlanningStore }` to `import type { PlanningStore }` (it's only used as a type in that file... wait: it IS used as `new PlanningStore(db)` on line 26. Check carefully before changing.)
- `execution.test.ts` lines 7-8: merge the two `import type { ... } from "./types.js"` statements into one.

**Actually re-examine `checkpoint.test.ts`:** The error is `consistent-type-imports` for line 12: `import { PlanningStore } from "./store.js"`. If `PlanningStore` is only used as a type annotation and not instantiated in that file, it should be `import type`. If it IS instantiated, this is a false positive or a different issue. **The integration test author must verify before making changes.**

**Warning signs:** PR CI failing on lint even after running clean locally (if the two test-file errors are not fixed).

### Pitfall 4: Spec number 31 confirmed but gap in sequence
**What goes wrong:** Specs jump from 30 to 31 with no 31 yet. The spec file should be `docs/specs/31_dianoia.md` (lowercase, underscore separator, matching convention of `30_homepage-dashboard.md`).
**Why it happens:** Non-issue — just a naming check.
**How to avoid:** Name file `31_dianoia.md`, not `31-dianoia.md` or `spec-31-dianoia.md`.

### Pitfall 5: Polling in PlanningPanel creates interval leak
**What goes wrong:** Svelte 5 `$effect` with `setInterval` must return a cleanup function. Missing the cleanup leaks the interval when the component unmounts.
**Why it happens:** Easy to forget the return value from `$effect`.
**How to avoid:** Always pattern:
```typescript
$effect(() => {
  const iv = setInterval(fn, 2500);
  return () => clearInterval(iv); // cleanup — required
});
```

---

## Code Examples

### makeDb() helper for integration test
```typescript
// Pattern from ALL existing dianoia unit tests — use exactly this
import Database from "better-sqlite3";
import {
  PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION,
  PLANNING_V23_MIGRATION, PLANNING_V24_MIGRATION, PLANNING_V25_MIGRATION,
} from "./schema.js";

function makeDb(): Database.Database {
  const db = new Database(":memory:");
  db.pragma("journal_mode = WAL");
  db.pragma("foreign_keys = ON");
  db.exec(PLANNING_V20_DDL);
  db.exec(PLANNING_V21_MIGRATION);
  db.exec(PLANNING_V22_MIGRATION);
  db.exec(PLANNING_V23_MIGRATION);
  db.exec(PLANNING_V24_MIGRATION);
  db.exec(PLANNING_V25_MIGRATION);
  return db;
}
```

### Mocked dispatchTool for integration test
```typescript
// For research/execution/verification dispatch
import { vi } from "vitest";
import type { ToolHandler, ToolContext } from "../organon/registry.js";

function makeDispatchTool(resultOverride?: object): ToolHandler {
  return {
    definition: {
      name: "sessions_dispatch",
      description: "mock",
      input_schema: { type: "object", properties: {}, required: [] },
    },
    execute: vi.fn().mockResolvedValue(
      JSON.stringify({
        results: [
          {
            status: "success",
            result: JSON.stringify(resultOverride ?? {
              status: "met",
              summary: "All criteria met",
              gaps: [],
            }),
            durationMs: 10,
          },
        ],
      }),
    ),
  } as unknown as ToolHandler;
}
```

### ToolContext stub for integration test
```typescript
// Minimal ToolContext for orchestrator calls
const mockContext: ToolContext = {
  nousId: "nous-test",
  sessionId: "session-test",
  sessionKey: "signal-key",
};
```

### Fix for execution.test.ts no-duplicates error
```typescript
// Before (2 separate imports):
import type { PlanningPhase } from "./types.js";
import type { SpawnRecord } from "./types.js";

// After (merge into one):
import type { PlanningPhase, SpawnRecord } from "./types.js";
```

### Fix for checkpoint.test.ts consistent-type-imports error
First verify: `PlanningStore` is imported on line 12. Check if it's used as `new PlanningStore(db)` or only as a type. In `checkpoint.test.ts` line 26+, `PlanningStore` IS used as `new PlanningStore(db)` — so it should NOT be `import type`. The oxlint error on line 12 may refer to something else. **Verify by reading the exact error context: the error says line 12 `import { PlanningStore } from "./store.js"` — if PlanningStore is only used as a TYPE annotation (not instantiated), change to `import type`. If it IS instantiated, the oxlint finding is wrong. Resolve by inspection.**

### API routes confirmed from routes.ts
```
GET  /api/planning/projects
     → 200: { id, goal, state, createdAt, updatedAt }[]
     → 503: { error: "Planning not enabled" }

GET  /api/planning/projects/:id
     → 200: { id, nousId, sessionId, goal, state, config, projectContext, contextHash, createdAt, updatedAt }
     → 404: { error: "Project not found" }
     → 503: { error: "Planning not enabled" }

GET  /api/planning/projects/:id/roadmap
     → 200: { projectId, state, phases: [{ id, name, goal, requirements, successCriteria, phaseOrder, status, hasPlan }] }
     → 404: { error: "Project not found" }
     → 503: { error: "Planning not enabled" }

GET  /api/planning/projects/:id/execution
     → 200: ExecutionSnapshot { projectId, state, activeWave, plans, activePlanIds, startedAt, completedAt }
     → 404: { error: "Project not found" }
     → 503: { error: "Planning not enabled" | "Execution orchestrator not available" }

GET  /api/planning/projects/:id/phases/:phaseId/status
     → 200: { phaseId, projectId, status, waveCount, currentWave, plans: PlanEntry[] }
     → 404: { error: "Project not found" }
     → 503: { error: "Planning not enabled" | "Execution orchestrator not available" }
```

### Full SQLite DDL (v20-v25) for spec document
All DDL is in `infrastructure/runtime/src/dianoia/schema.ts`:
- **V20 (base)**: 5 tables — `planning_projects`, `planning_phases`, `planning_requirements`, `planning_checkpoints`, `planning_research` + 5 indexes
- **V21**: `ALTER TABLE planning_projects ADD COLUMN project_context TEXT`
- **V22**: `ALTER TABLE planning_research ADD COLUMN status TEXT NOT NULL DEFAULT 'complete' CHECK(...)`
- **V23**: `ALTER TABLE planning_requirements ADD COLUMN rationale TEXT`
- **V24**: New table `planning_spawn_records` + 2 indexes
- **V25**: 4 ALTER statements — `planning_checkpoints` gets `risk_level`, `auto_approved`, `user_note`; `planning_phases` gets `verification_result`

---

## State of the Art

| Area | Current State | Notes |
|------|---------------|-------|
| TypeScript | `tsc --noEmit` clean (0 errors, 0 warnings) | Test files excluded from tsconfig — lint errors in test files only visible via oxlint |
| oxlint | 2 errors, 144 warnings — all pre-existing | 2 errors in dianoia test files (trivial); 144 warnings spread across unrelated modules |
| Spec numbering | Last spec is 30_homepage-dashboard.md | Next number is 31; file naming convention is `NN_slug.md` (underscore) |
| Existing unit tests | 194 tests green | All 11 dianoia unit test files pass; integration test file does not yet exist |
| UI panels | ThinkingPanel + ToolPanel patterns established | PlanningPanel follows same pattern (380px, slide-in, border-left) |

---

## Open Questions

1. **checkpoint.test.ts `consistent-type-imports` error — is PlanningStore instantiated or type-only?**
   - What we know: oxlint flags line 12 `import { PlanningStore } from "./store.js"` as consistent-type-imports error
   - What's unclear: if PlanningStore is only used as a type annotation (fix: `import type`), or if it's instantiated (fix: add `/* PlanningStore is instantiated */` comment to suppress, or check if oxlint version changed)
   - Recommendation: Read checkpoint.test.ts lines 12-30 carefully. `makeDb()` creates a `PlanningStore(db)` — that IS a value use, not just a type. If oxlint is wrong, suppress with `// eslint-disable-next-line` or check the exact rule interpretation. Most likely resolution: the error is on a different import in that statement. Read the exact error output carefully at fix time.

2. **Should integration test cover any failure path?**
   - What we know: CONTEXT.md says happy path is minimum; "Claude decides" additional coverage based on gaps in unit tests
   - What's unclear: are cascade-skip and zombie detection covered in existing unit tests?
   - Recommendation: `execution.test.ts` already has cascade-skip and zombie tests. The integration test should add at least one failure path not covered by unit tests — specifically: a phase fails during execution and the project ends up in `blocked` state (verifies the FSM + store integration across multiple components).

3. **Does the status pill show up in the InputBar area or in the MessageList?**
   - What we know: CONTEXT.md says "a status pill in the chat interface" — similar to tool-use/thinking pills
   - What's unclear: exact placement — next to InputBar (always visible) vs. in the MessageList (per-message context)
   - Recommendation: The existing ToolStatusLine and ThinkingStatusLine appear inside `MessageList.svelte` per-message. For planning status, the pill should appear in `ChatView.svelte` above the InputBar as a persistent global indicator (not per-message) — similar to `DistillationProgress.svelte`. Only show when an active planning project exists for the current nous.

---

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | vitest (version in package.json) |
| Config file | `infrastructure/runtime/vitest.config.ts` (inferred from existing test runs) |
| Quick run command | `npx vitest run src/dianoia/dianoia.integration.test.ts` |
| Full dianoia suite | `npx vitest run src/dianoia/` |
| Full suite | `npx vitest run` |
| Estimated runtime | ~5-10 seconds for dianoia suite |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DOCS-01 | Spec doc exists and follows format | manual | `ls docs/specs/31_dianoia.md` | No — Wave 0 gap |
| DOCS-02 | Spec includes DDL, state diagram, API surface | manual | review file content | No — Wave 0 gap |
| DOCS-03 | CONTRIBUTING.md has Dianoia section with 4 gotchas | manual | `grep "Dianoia" CONTRIBUTING.md` | No — Wave 0 gap |
| TEST-04 | Integration test: idle→complete happy path | integration | `npx vitest run src/dianoia/dianoia.integration.test.ts` | No — Wave 0 gap |
| TEST-05 | `tsc --noEmit` exits 0 | typecheck | `npx tsc --noEmit` | N/A (runs against src/) |
| TEST-06 | `npx oxlint src/` exits 0 errors | lint | `npx oxlint src/` | N/A (runs against src/) |

### Nyquist Sampling Rate
- **Minimum sample interval:** After each task → run: `npx vitest run src/dianoia/dianoia.integration.test.ts && npx tsc --noEmit`
- **Full suite trigger:** Before final task completion
- **Phase-complete gate:** `npx vitest run src/dianoia/ && npx tsc --noEmit && npx oxlint src/` all green
- **Estimated feedback latency per task:** ~15 seconds

### Wave 0 Gaps
- [ ] `src/dianoia/dianoia.integration.test.ts` — covers TEST-04 (happy path integration test)
- [ ] `docs/specs/31_dianoia.md` — covers DOCS-01 and DOCS-02
- [ ] `CONTRIBUTING.md` update — covers DOCS-03
- [ ] Two lint fixes in existing test files (checkpoint.test.ts + execution.test.ts) — covers TEST-06 prerequisites
- [ ] `ui/src/components/chat/PlanningStatusLine.svelte` — status pill UI (no requirement ID, scoped to Phase 9 via CONTEXT.md)
- [ ] `ui/src/components/chat/PlanningPanel.svelte` — right-pane execution panel

---

## Sources

### Primary (HIGH confidence)
- `infrastructure/runtime/src/dianoia/machine.ts` — all 11 states, 13 events, all transitions verified by reading source
- `infrastructure/runtime/src/dianoia/schema.ts` — complete DDL for all 5 tables + 6 migration steps verified
- `infrastructure/runtime/src/dianoia/routes.ts` — all 5 routes with exact response shapes verified
- `infrastructure/runtime/src/dianoia/execution.ts` — ExecutionSnapshot and PlanEntry interfaces
- `infrastructure/runtime/src/dianoia/orchestrator.ts` — all public methods (15 methods) for spec API section
- `infrastructure/runtime/src/dianoia/*.test.ts` (11 files) — makeDb() pattern, existing test coverage verified
- `ui/src/components/chat/ToolStatusLine.svelte` — pill CSS/structure pattern
- `ui/src/components/chat/ThinkingStatusLine.svelte` — pill with left-border-active pattern
- `ui/src/components/chat/ToolPanel.svelte` — right pane structure, expand/collapse pattern
- `ui/src/components/chat/ThinkingPanel.svelte` — right pane structure, 380px/slide-in pattern
- `ui/src/components/chat/ChatView.svelte` — panel wiring pattern (selectedTools, selectedThinking state)
- `docs/specs/_template.md` — spec template format
- `docs/specs/29_ui-layout-and-theming.md` — current spec format example
- `docs/specs/30_homepage-dashboard.md` — current spec format example
- `CONTRIBUTING.md` — full file read, confirmed section structure
- `.planning/config.json` — nyquist_validation not set (treated as false; Validation Architecture section included anyway per task scope)
- `infrastructure/runtime/tsconfig.json` — test files excluded from tsc, exactOptionalPropertyTypes enabled
- oxlint output: 2 errors (both in dianoia test files), 144 warnings (all pre-existing in other modules)
- tsc output: 0 errors, 0 warnings (clean)

### Secondary (MEDIUM confidence)
- ChatView.svelte polling pattern: inferred from existing `pollInterval` for SSE disconnection — confirms setInterval is the project pattern for polling
- Status pill placement: inferred from DistillationProgress positioning in ChatView — supports "above InputBar as persistent indicator" recommendation

---

## Metadata

**Confidence breakdown:**
- Spec content (states, DDL, routes): HIGH — all sourced directly from code
- Integration test pattern: HIGH — directly derived from 11 existing unit tests
- Status pill UI pattern: HIGH — ThinkingStatusLine and ToolPanel are exact templates
- Polling vs SSE decision: HIGH — verified pylon SSE stream does not emit planning events to UI
- Lint fix for execution.test.ts: HIGH — no-duplicates error is clear, fix is merge two import lines
- Lint fix for checkpoint.test.ts: MEDIUM — need to verify if PlanningStore is value-used or type-only in that file before changing import style

**Research date:** 2026-02-24
**Valid until:** Stable — all sources are project source files (no external dependencies)
