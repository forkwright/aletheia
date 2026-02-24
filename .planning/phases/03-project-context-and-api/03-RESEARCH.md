# Phase 3: Project Context & API - Research

**Researched:** 2026-02-24
**Domain:** Dianoia conversational context gathering, Hono pylon API routes, event bus integration, working-state injection, legacy tool deprecation
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Questioning flow:**
- One question at a time by default — adaptive, conversational
- Exception: 2-3 closely related questions may be grouped in one message when they naturally belong together (e.g., "What's the tech stack?" and "Any external APIs involved?")
- Questions adapt based on prior answers — not a fixed script
- Primary delivery is inline in the conversation (existing channel)

**Context synthesis depth (calibrated to project scope):**
- Agent summarizes gathered context and asks for confirmation before saving
- Synthesis depth scales with the problem:
  - Tactical (simple script, one-off task): brief summary of goal and constraints
  - Strategic (table redesign, data model, system architecture): explicit coverage of both tactical (what we're building) AND strategic (why, risks, alternatives considered, long-term implications)
- The nous should genuinely think at the level the problem deserves — not a rote summary
- Confirmation question: "Here's what I captured: [summary] — does this look right?" before state transitions to the next phase

**Context fields captured:**
- `goal` — one-sentence statement of what's being built and why
- `coreValue` — what this delivers (optional, for strategic work)
- `constraints` — hard limits (tech, time, compatibility)
- `keyDecisions` — architectural/approach choices already made
- `rawTranscript` — the conversation excerpts that informed synthesis (for audit)
- Free-form fields are fine — not every project has all fields

**API response shape:**
- `GET /api/planning/projects` — array of projects (id, goal, state, createdAt, updatedAt)
- `GET /api/planning/projects/:id` — full snapshot: all fields including synthesized context (goal, coreValue, constraints, keyDecisions, config, state, contextHash, createdAt, updatedAt)
- No tiered/partial responses — full snapshot on `:id` route

**Legacy tool deprecation:**
- `plan_propose` and `plan_create`: both get JSDoc `@deprecated` annotation with migration note pointing to `/plan` command
- Both also emit a runtime warning in their tool output text: "Warning: Deprecated: use /plan instead"
- Tools remain functional — not removed, just clearly marked
- `plan_status`, `plan_step_complete`, `plan_step_fail` deprecation is Phase 9 (not this phase)

### Claude's Discretion
- Whether to use `AskUserQuestion`-style structured prompts for gathering (interview UI concept) vs inline prose questions — explore this if the pylon/channel architecture supports it, otherwise inline prose is fine
- Exact number of questions to ask (3-5 is the natural range; agent judges based on project complexity)
- How to detect project complexity for synthesis depth calibration (simple heuristic: look at the problem domain, number of constraints mentioned, presence of strategic keywords)

### Deferred Ideas (OUT OF SCOPE)
- Dedicated "interview mode" UI (AskUserQuestion-style structured gathering) — worth a dedicated Phase if pylon supports it; not blocking Phase 3's inline approach
- `plan_status`/`plan_step_complete`/`plan_step_fail` deprecation — Phase 9 (Polish & Migration)
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| PROJ-01 | User can answer questions about their project through natural conversation (agent asks, user answers inline) | Orchestrator already opens with first question; needs questioning-driver logic that asks follow-up questions per turn |
| PROJ-02 | Project context is persisted to SQLite after questioning phase completes | PlanningStore.updateProjectConfig() and a new updateProjectGoal() method cover the JSON fields; questioning loop must call these before transitioning |
| PROJ-03 | Agent synthesizes project description, core value, constraints, and key decisions from the conversation | Fields map directly to PlanningProject.config extended with coreValue/constraints/keyDecisions/rawTranscript keys |
| PROJ-04 | Project context is injected into agent working-state and survives distillation | System prompt injection already exists (context.ts Active Dianoia Planning Project block); needs enrichment with synthesized fields; working-state extraction already runs post-turn |
| INTG-01 | Dianoia exposes /api/planning/projects CRUD routes in pylon | Pattern is clear: add planningRoutes factory in dianoia/routes.ts, import and mount in server.ts modules array |
| INTG-02 | Planning project state is accessible via GET /api/planning/projects/:id | PlanningStore.getProjectOrThrow() + PlanningStore.listProjects() already exist; route maps them to JSON |
| INTG-03 | Planning fires events on Aletheia's event bus (planning:project-created, planning:phase-started, planning:phase-complete, planning:checkpoint, planning:complete) | EventName union already has all 5 events; need emit calls in orchestrator for phase-started, phase-complete, checkpoint, complete |
| INTG-04 | Planning state is injected into agent working-state so the nous knows the active planning context | context.ts injection block exists but only shows id/state/goal; needs enrichment with synthesized context fields |
| INTG-05 | Existing plan_create/plan_propose tools are marked deprecated but not removed (documented migration path to Dianoia tools) | Both tools are in organon/built-in/; JSDoc @deprecated + prepend warning string to execute() return value |
</phase_requirements>

---

## Summary

Phase 3 builds on a solid foundation: PlanningStore (all CRUD), DianoiaOrchestrator (handle/abandon/confirmResume), FSM transitions, and the context injection infrastructure are all complete and tested. The orchestrator already transitions to `questioning` state on project creation and returns the first question string — but nothing drives subsequent questions. Phase 3 must extend the orchestrator to process conversational answers, synthesize context into structured fields, persist to SQLite, and confirm with the user before advancing.

The pylon route pattern is clear from reading `plans.ts` and `server.ts`. A new `planningRoutes` factory in `dianoia/routes.ts` follows identical shape — a `Hono` instance, route handlers using `PlanningStore` directly (passed via a new `RouteDeps` field or accessed through `NousManager`). The `createGateway` function mounts route modules by calling factory functions in a `modules` array; adding planning routes requires one import and one array entry.

Working-state injection already runs post-turn via `extractWorkingState` (cheap model extraction) stored in `sessions.working_state`. Planning context is already injected into the system prompt each turn from `context.ts`. The challenge for PROJ-04 is that synthesized planning fields (goal, coreValue, constraints, keyDecisions) live in `planning_projects.config` JSON — the context block just needs to read these and format them. Distillation survival is already handled: the Active Dianoia Planning Project block is injected fresh from the DB on every turn (not from history), so it persists through any distillation automatically.

**Primary recommendation:** Extend DianoiaOrchestrator with a `processAnswer(projectId, userText)` method that drives the questioning loop; add `planningRoutes` factory following the exact plans.ts pattern; enrich the context.ts planning block with synthesized fields; emit the 3 missing events (phase-started, phase-complete, complete) from the orchestrator; prepend the deprecation warning string to plan_propose and plan_create execute() returns.

---

## Standard Stack

### Core (no new dependencies)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| better-sqlite3 | 12.6.2 | SQLite reads/writes via PlanningStore | Already in use; all 5 planning tables exist |
| Hono | existing | HTTP route handlers | All pylon routes use Hono; identical pattern |
| zod | 3.25.76 | Schema validation for API response shapes | Used throughout taxis/schema.ts |
| eventBus | existing | planning:* event emission | Type-safe; all 5 EventName entries already declared |

**No npm install needed.** Zero new dependencies.

---

## Architecture Patterns

### Existing Module Structure (Phase 3 extends this)

```
dianoia/
  index.ts           — Public API barrel (add routes export)
  machine.ts         — Pure FSM (already complete)
  orchestrator.ts    — Extend with processAnswer(), synthesize(), confirmSynthesis()
  store.ts           — Extend with updateProjectGoal(), updateProjectContext()
  schema.ts          — DDL already complete
  types.ts           — Extend PlanningProject or use config JSON for context fields
  routes.ts          — NEW: planningRoutes(deps, refs, planningOrchestrator): Hono
  store.test.ts      — Extend with context field tests
  orchestrator.test.ts — Extend with questioning-loop tests
```

### Pattern 1: Pylon Route Module

All pylon routes follow exactly this shape (verified from `plans.ts` and `sessions.ts`):

```typescript
// Source: infrastructure/runtime/src/pylon/routes/plans.ts
import { Hono } from "hono";
import { createLogger } from "../../koina/logger.js";
import type { RouteDeps, RouteRefs } from "./deps.js";

const log = createLogger("pylon");

export function planningRoutes(
  deps: RouteDeps,
  _refs: RouteRefs,
  planningOrchestrator: DianoiaOrchestrator,  // injected extra
): Hono {
  const app = new Hono();
  // deps.store is SessionStore (not PlanningStore)
  // planningOrchestrator.getStore() or pass PlanningStore separately

  app.get("/api/planning/projects", (c) => {
    const projects = planningOrchestrator.listAllProjects();
    return c.json(projects.map(p => ({
      id: p.id, goal: p.goal, state: p.state,
      createdAt: p.createdAt, updatedAt: p.updatedAt,
    })));
  });

  app.get("/api/planning/projects/:id", (c) => {
    const project = planningOrchestrator.getProject(c.req.param("id"));
    if (!project) return c.json({ error: "Not found" }, 404);
    return c.json(project);  // full snapshot
  });

  return app;
}
```

**Critical constraint:** `RouteDeps` does not currently include `planningOrchestrator`. There are two options:
1. Add `planningOrchestrator?: DianoiaOrchestrator` to `RouteDeps` in `deps.ts` (cleanest)
2. Pass it as extra parameter to factory (non-standard but avoids deps.ts change)

**Recommended: Option 1** — add to `RouteDeps`. This is the right place for it; the field is optional so existing route modules are unaffected. Then wire it in `createGateway()` in `server.ts` by extracting the orchestrator from manager.

### Pattern 2: Route Registration in server.ts

```typescript
// Source: infrastructure/runtime/src/pylon/server.ts lines 183-208
// Current modules array:
const modules = [setupRoutes, systemRoutes, authRoutes, /* ...18 more */];
for (const factory of modules) {
  app.route("", factory(deps, refs));
}
```

**Problem:** The current loop calls `factory(deps, refs)` — if `planningRoutes` needs `planningOrchestrator`, either:
- Add it to `deps` (recommended), then factory signature stays `(deps, refs) => Hono`
- Or call it separately: `app.route("", planningRoutes(deps, refs))` where `deps` now includes orchestrator

The cleanest approach: add `planningOrchestrator?: DianoiaOrchestrator` to `RouteDeps` and set it in `createGateway()` by calling `manager.getPlanningOrchestrator()` (the getter already exists on NousManager).

### Pattern 3: Orchestrator Questioning Loop

The current `handle()` creates a project, transitions to `questioning`, and returns the first question string. The **existing design** is that the orchestrator's `handle()` return value is sent directly as the agent's response text via the `/plan` command execute() and by the context injection system.

Phase 3 needs a new path: when the agent is in `questioning` state and the user sends a message, the turn pipeline needs to recognize it as a planning answer and route it to the orchestrator. This is the key architectural decision:

**Option A: Context injection drives it** (recommended for inline approach)
The `context.ts` planning block already injects the current state. The agent reads the state and continues asking questions naturally (LLM-driven). The orchestrator's role is to persist answers and advance state. A `processAnswer(projectId, nousId, userText)` method:
1. Appends to rawTranscript in config
2. Returns null (let the LLM formulate the next question)
3. Eventually the agent calls a tool or the context block signals "questioning complete"

**Option B: A new tool `dianoia_answer` captures structured answers** — but this conflicts with the inline conversational approach locked in CONTEXT.md.

**Option C: Orchestrator drives the questioning with explicit prompts** — orchestrator returns a "next question" string to be injected into context, agent reads it and delivers it verbatim.

**Recommended: Hybrid of A and C.** The orchestrator exposes a `getNextQuestion(projectId)` method that returns the next question to ask (or null if enough gathered). The context injection block includes this question as a `## Planning Question` block. The LLM delivers it naturally. After the user responds, the next turn's `buildContext()` call processes the previous turn's user message as an answer (via a new `recordAnswer(projectId, userText)` method). When the orchestrator determines enough context is gathered, it synthesizes and presents for confirmation.

### Pattern 4: Context Fields — Where to Store Them

`PlanningProject` already has `config: PlanningConfig` (JSON column). The `PlanningConfig` type is `PlanningConfigSchema` from taxis — a Zod schema covering depth/parallelization/research/plan_check/verifier/mode. The phase-specific context fields (coreValue, constraints, keyDecisions, rawTranscript) do NOT fit in config — they are project data, not configuration.

**Two options:**
1. Extend `planning_projects` table with new columns (requires migration v21)
2. Store in the existing `config` JSON as extended fields via cast-through-unknown (same pattern used for `pendingConfirmation`)

**Verified existing pattern from orchestrator.ts:**
```typescript
// Source: infrastructure/runtime/src/dianoia/orchestrator.ts lines 32-34
const updated = { ...(active.config as Record<string, unknown>), pendingConfirmation: true };
this.store.updateProjectConfig(active.id, updated as unknown as PlanningConfigSchema);
```

**Recommended: Add a `context` JSON column to `planning_projects` via migration approach.** This is cleaner than stuffing data into config. However, a migration bump to v21 requires adding a new migration entry. Alternatively, store the context fields in a new `projectContext` key inside the config JSON to avoid a schema change.

**The simplest correct approach:** Add `projectContext?: ProjectContext` as an extended config key (no migration), with type `ProjectContext = { goal?: string; coreValue?: string; constraints?: string[]; keyDecisions?: string[]; rawTranscript?: string[] }`. Note: `planning_projects.goal` is already a top-level column and should be updated via `updateProjectGoal()` when synthesized. The richer fields (coreValue, constraints, keyDecisions, rawTranscript) go in config as `projectContext`.

**Alternatively:** The cleanest approach that avoids config pollution: update `planning_projects.goal` (existing column) for the goal, and store the structured context in a new `project_context` TEXT column via migration v21. Migration v20 is already applied; v21 is a simple `ALTER TABLE planning_projects ADD COLUMN project_context TEXT`.

**Recommendation:** Add migration v21 with `ALTER TABLE` — clean separation, no type casting, proper mapper.

### Pattern 5: Event Emission

Current `eventBus` pattern (verified from `orchestrator.ts` and `event-bus.ts`):

```typescript
// Source: infrastructure/runtime/src/dianoia/orchestrator.ts line 44
eventBus.emit("planning:project-created", { projectId: project.id, nousId, sessionId });

// Source: infrastructure/runtime/src/koina/event-bus.ts line 55
emit(event: EventName, payload: EventPayload): void  // EventPayload = Record<string, unknown>
```

Missing events to emit (INTG-03):
- `planning:phase-started` — emit when FSM transitions to `researching` (first phase after questioning)
- `planning:phase-complete` — emit when a phase finishes
- `planning:checkpoint` — emit when human confirmation is requested (synthesis confirmation counts)
- `planning:complete` — emit when FSM reaches `complete` state

All 5 EventName entries are already declared in `event-bus.ts` (lines 26-30, verified). Only emission calls are missing.

### Pattern 6: Legacy Tool Deprecation

`plan_propose` is in `organon/built-in/plan-propose.ts`. It exports `createPlanProposeHandler()` which returns a `ToolHandler` with an `execute` async function that returns a JSON string.

`plan_create` is in `organon/built-in/plan.ts`. It exports `createPlanTools(store)` which returns `ToolHandler[]` — the first element is `plan_create`.

**Deprecation approach (verified from source):**

```typescript
// plan-propose.ts — prepend to the JSON return in execute():
export function createPlanProposeHandler(): ToolHandler {
  return {
    definition: {
      name: "plan_propose",
      // Add JSDoc @deprecated above the function
      description: /* existing description */ + "\n\nDEPRECATED: Use /plan command instead.",
    },
    async execute(input, context): Promise<string> {
      // ... existing logic ...
      const result = JSON.stringify({ __marker: PLAN_PROPOSED_MARKER, plan: { ... } });
      // Prepend warning to tool output text
      return "Warning: Deprecated: use /plan instead.\n\n" + result;
      // NOTE: The tool output is parsed as JSON by the execute stage only if __marker is present.
      // Prepending text breaks JSON.parse — use a different approach:
    }
  };
}
```

**CRITICAL PITFALL:** `plan_propose`'s execute() output is parsed by `execute.ts` for the `__marker: PLAN_PROPOSED_MARKER` pattern. Prepending text before the JSON would break this. The warning must be inside the JSON structure, or the tool must return a separate text response.

**Correct approach for plan_propose:** Return a two-part response or modify the marker check to handle prefixed text. The cleanest fix: add a `deprecationWarning` field inside the JSON payload instead of prepending text.

**Correct approach for plan_create:** Its output is just JSON read by the agent. Prepending text is safe here, but the agent might not display it. Better: add `deprecationWarning` field to the JSON response.

**Alternative (simplest):** Update the `description` field to include the deprecation note prominently — the description is what Claude reads when deciding to call the tool. Add JSDoc `@deprecated` comment above the handler. The runtime warning in tool output can be a `warning` key in the JSON response, which Claude will include in its response to the user.

### Pattern 7: Working-State and Distillation Survival (PROJ-04)

The current `## Active Dianoia Planning Project` system prompt block (context.ts lines 161-166) is injected from the DB on every turn. It shows: `Project ID`, `State`, `Goal`. It does NOT depend on conversation history, so it survives distillation automatically.

For PROJ-04 enrichment: when project context is synthesized, the same block should also show `Core Value`, `Constraints`, `Key Decisions`. These fields come from the DB, not from conversation history — so distillation survival is already guaranteed by the injection pattern.

**What PROJ-04 specifically needs:**
- Update the context injection block to include synthesized fields when available
- Confirm that `extractWorkingState()` (post-turn cheap extraction) captures the planning task as `currentTask` — this is already wiring via the `## Working State` block, not planning-specific

**No changes to the distillation pipeline needed.** The planning context injection pattern already provides distillation survival.

### Anti-Patterns to Avoid

- **Putting context fields in `planning_projects.config`:** The config column is for planning configuration (depth, mode, etc.), not project content. Polluting it requires type casts and makes the schema unclear.
- **Storing rawTranscript as a single concatenated string:** It should be a JSON array of `{ turn: number; text: string }` objects for audit use.
- **Emitting events synchronously in the middle of DB transactions:** `eventBus.emit()` is sync but handlers may be async. Emit after the transaction completes, not inside it.
- **Breaking plan_propose's JSON parsing:** The `execute.ts` stage parses the __marker — any text before the JSON breaks it. Warning must be inside the JSON payload.
- **Adding planningOrchestrator to RouteDeps as required:** Make it optional to avoid breaking existing route modules and tests.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| HTTP routing | Custom router | Hono (existing) | All pylon routes use Hono; identical pattern |
| DB transactions | Manual BEGIN/COMMIT | `db.transaction()` (better-sqlite3) | Already used everywhere in PlanningStore |
| Event emission typing | Custom type guards | EventName union (existing) | All 5 planning: events already declared |
| JSON column mapping | Ad-hoc JSON.parse | PlanningStore private mappers | Existing PLANNING_STATE_CORRUPT error handling |
| Schema migration | In-code ALTER | mneme/schema.ts MIGRATIONS array | Existing migration runner handles versioning |

**Key insight:** This phase is almost entirely integration work — wiring existing primitives together correctly, not building new infrastructure.

---

## Common Pitfalls

### Pitfall 1: plan_propose JSON marker collision
**What goes wrong:** Prepending deprecation text before the JSON string returned from `plan_propose.execute()` breaks the `execute.ts` marker check that looks for `__marker: PLAN_PROPOSED_MARKER` via JSON.parse.
**Why it happens:** The tool's output text is parsed as JSON by the pipeline stage to detect plan proposals — it's not just displayed to the user.
**How to avoid:** Add a `deprecationWarning` key inside the existing JSON payload. The agent receives it in the tool result and will mention it in its response.
**Warning signs:** `execute.ts` silently swallows the plan proposal — plan is never stored.

### Pitfall 2: RouteDeps not updated before adding planningRoutes to modules array
**What goes wrong:** `planningRoutes(deps, refs)` factory can't access `planningOrchestrator` without it being in `deps`.
**Why it happens:** `RouteDeps` interface in `deps.ts` is the only injection point for route modules — the modules array loop calls `factory(deps, refs)` with no other arguments.
**How to avoid:** Add `planningOrchestrator?: DianoiaOrchestrator` to `RouteDeps` interface first, then set it in `createGateway()` via `manager.getPlanningOrchestrator()`.
**Warning signs:** TypeScript error on `deps.planningOrchestrator` access.

### Pitfall 3: Questioning state never exits
**What goes wrong:** The orchestrator transitions to `questioning` state but no mechanism drives the transition to `researching` — project stays in questioning forever.
**Why it happens:** The FSM transition must be triggered explicitly; nothing calls `transition("questioning", "COMPLETE_QUESTIONING")` unless the orchestrator does it after synthesis confirmation.
**How to avoid:** The synthesis confirmation path (`confirmSynthesis()`) must call `store.updateProjectState(projectId, transition("questioning", "COMPLETE_QUESTIONING"))` and emit `planning:phase-started`.
**Warning signs:** Project state stays `questioning` after user confirms synthesis.

### Pitfall 4: context.ts injection not showing synthesized fields
**What goes wrong:** The Active Dianoia Planning Project block shows id/state/goal but not coreValue/constraints/keyDecisions after synthesis.
**Why it happens:** The block was built in Phase 2 with minimal fields; new context fields need to be read from the DB and formatted.
**How to avoid:** When `getActiveProject()` returns a project, read the `projectContext` field from config and include it in the block format.
**Warning signs:** INTG-04 verification fails — agent doesn't know constraints/decisions.

### Pitfall 5: Migration v21 not run at startup
**What goes wrong:** `ALTER TABLE planning_projects ADD COLUMN project_context TEXT` exists in migration v21 but isn't applied — PlanningStore mapper crashes on the new field.
**Why it happens:** Migration runner only applies migrations once; if v21 is added but the migration runner hasn't run, existing DBs don't have the column.
**How to avoid:** Follow the exact pattern in `mneme/schema.ts` — add a `{ version: 21, sql: "ALTER TABLE planning_projects ADD COLUMN project_context TEXT" }` entry. Test with in-memory DB where all migrations run from scratch.
**Warning signs:** `no such column: project_context` SQLite error on first project read after migration.

### Pitfall 6: EventName missing planning:checkpoint in VALID_EVENTS
**What goes wrong:** Hook YAML validation rejects `planning:checkpoint` event because it's not in `VALID_EVENTS` set in `koina/hooks.ts`.
**Why it happens:** VALID_EVENTS must mirror EventName — but Phase 2 only added the 5 events already in the union, and `planning:checkpoint` IS already declared (lines 253-254 of hooks.ts, per verification report).
**How to avoid:** Verify `VALID_EVENTS` has all 5 events before emitting. Per Phase 2 verification, all 5 ARE already there.
**Warning signs:** TypeScript/runtime error on `eventBus.emit("planning:checkpoint", ...)`.

---

## Code Examples

### Route Module Pattern (verified from plans.ts)

```typescript
// Source: infrastructure/runtime/src/pylon/routes/plans.ts — exact pattern to follow
import { Hono } from "hono";
import { createLogger } from "../../koina/logger.js";
import type { RouteDeps, RouteRefs } from "./deps.js";

const log = createLogger("pylon");

export function planningRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();

  // deps.planningOrchestrator is the access path (after RouteDeps update)
  const orch = deps.planningOrchestrator;

  app.get("/api/planning/projects", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    const projects = orch.listAllProjects();
    return c.json(projects.map(p => ({
      id: p.id, goal: p.goal, state: p.state,
      createdAt: p.createdAt, updatedAt: p.updatedAt,
    })));
  });

  app.get("/api/planning/projects/:id", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    const project = orch.getProject(c.req.param("id"));
    if (!project) return c.json({ error: "Project not found" }, 404);
    return c.json(project);  // full snapshot — no field omission
  });

  return app;
}
```

### RouteDeps Extension Pattern (verified from deps.ts)

```typescript
// Source: infrastructure/runtime/src/pylon/routes/deps.ts
import type { DianoiaOrchestrator } from "../../dianoia/index.js";

export interface RouteDeps {
  config: AletheiaConfig;
  manager: NousManager;
  store: SessionStore;
  authConfig: AuthConfig;
  authSessionStore: AuthSessionStore | null;
  auditLog: AuditLog | null;
  authRoutes: { /* ... */ };
  planningOrchestrator?: DianoiaOrchestrator;  // ADD THIS
}
```

### createGateway Wiring (verified from server.ts)

```typescript
// Source: infrastructure/runtime/src/pylon/server.ts lines 164-180
const deps: RouteDeps = {
  config, manager, store, authConfig, authSessionStore, auditLog, authRoutes: authRouteFns,
  planningOrchestrator: manager.getPlanningOrchestrator() ?? undefined,  // ADD THIS
};
```

And add planningRoutes to the modules array:
```typescript
import { planningRoutes } from "../dianoia/routes.js";

const modules = [
  setupRoutes, systemRoutes, authRoutes, /* ... existing 18 ... */
  planningRoutes,  // ADD — after existing modules
];
```

### Event Emission Pattern (verified from orchestrator.ts)

```typescript
// Source: infrastructure/runtime/src/dianoia/orchestrator.ts line 44
// Pattern for new events:
eventBus.emit("planning:checkpoint", {
  projectId,
  nousId,
  type: "synthesis-confirmation",
  question: "Here's what I captured: [summary] — does this look right?",
});

eventBus.emit("planning:phase-started", {
  projectId,
  nousId,
  fromState: "questioning",
  toState: "researching",
});

eventBus.emit("planning:phase-complete", {
  projectId,
  nousId,
  phase: "questioning",
});
```

### Deprecation Pattern for plan_create (safe — JSON output consumed by agent)

```typescript
// Source: infrastructure/runtime/src/organon/built-in/plan.ts (verified)
// In the planCreate execute() body, before return:
return JSON.stringify({
  planId,
  stepCount: steps.length,
  actionableNow: actionable,
  deprecationWarning: "Deprecated: use /plan instead.",  // ADD THIS
});
```

### Deprecation Pattern for plan_propose (unsafe to prepend text — must use JSON key)

```typescript
// Source: infrastructure/runtime/src/organon/built-in/plan-propose.ts lines 103-116
// The __marker pattern is parsed by execute.ts — must keep JSON valid
return JSON.stringify({
  __marker: PLAN_PROPOSED_MARKER,
  deprecationWarning: "Deprecated: use /plan instead.",  // ADD INSIDE JSON
  plan: { id: planId, /* ... */ },
});
```

### context.ts Planning Block Enrichment Pattern

```typescript
// Source: infrastructure/runtime/src/nous/pipeline/stages/context.ts lines 161-166
// Current:
text: `## Active Dianoia Planning Project\n\nProject ID: ${activeProject.id}\nState: ${activeProject.state}\nGoal: ${activeProject.goal || "(not yet set)"}\n${hasPending ? "\nAwaiting resume confirmation from user." : ""}`,

// After PROJ-04 enrichment:
const ctx = (activeProject.config as Record<string, unknown>)["projectContext"] as ProjectContext | undefined;
text: `## Active Dianoia Planning Project\n\n` +
  `Project ID: ${activeProject.id}\n` +
  `State: ${activeProject.state}\n` +
  `Goal: ${activeProject.goal || "(not yet set)"}\n` +
  (ctx?.coreValue ? `Core Value: ${ctx.coreValue}\n` : "") +
  (ctx?.constraints?.length ? `Constraints: ${ctx.constraints.join("; ")}\n` : "") +
  (ctx?.keyDecisions?.length ? `Key Decisions: ${ctx.keyDecisions.join("; ")}\n` : "") +
  (hasPending ? "\nAwaiting resume confirmation from user." : ""),
```

### Migration v21 Pattern (following mneme/schema.ts pattern)

```typescript
// Source: infrastructure/runtime/src/mneme/schema.ts — MIGRATIONS array pattern
// Add after the existing v20 entry:
{
  version: 21,
  sql: `ALTER TABLE planning_projects ADD COLUMN project_context TEXT`,
},
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| plan_create/plan_propose for ad-hoc planning | DianoiaOrchestrator + /plan command | Phase 2 complete | Tools now deprecated; Dianoia is the canonical path |
| Working-state extracted post-turn | Working-state injected fresh every turn + planning context block | Phase 2 complete | Planning context survives distillation without special handling |
| No planning events on event bus | 5 planning EventName entries declared | Phase 2 complete | Only emission calls missing; all 5 events need emit() calls in Phase 3 |

**Deprecated/outdated:**
- `plan_propose`: functional but deprecated; Phase 3 adds deprecation markers
- `plan_create`: functional but deprecated; Phase 3 adds deprecation markers
- Inline `pendingConfirmation` in config: a short-term hack documented in Phase 2; Phase 3 should avoid extending this pattern for project context fields

---

## Open Questions

1. **Where exactly does the questioning loop get driven?**
   - What we know: `handle()` returns the first question string; the `/plan` command routes the return value to the agent as a response; the context block injects state on subsequent turns
   - What's unclear: On turn 2+ (user answers the first question), what mechanism causes the orchestrator to formulate the next question? Does the LLM do it naturally from the context block, or does the orchestrator provide an explicit next question?
   - Recommendation: Use the hybrid approach — context block includes `## Planning Question` with the next question string returned by `getNextQuestion(projectId)`. This is deterministic and testable. The LLM delivers it naturally.

2. **Where does `DianoiaOrchestrator` become accessible to route modules?**
   - What we know: `manager.getPlanningOrchestrator()` getter exists (verified in 02-01-SUMMARY); `RouteDeps` does not currently include it
   - What's unclear: Whether there's a cleaner pass-through path (e.g., via `NousManager` existing in `RouteDeps`)
   - Recommendation: Add `planningOrchestrator?: DianoiaOrchestrator` to `RouteDeps`, set via `manager.getPlanningOrchestrator()` in `createGateway()`.

3. **Should the synthesis confirmation be a checkpoint event or a special state?**
   - What we know: `planning:checkpoint` event is declared; CONTEXT.md says "confirmation question before state transitions"
   - What's unclear: Whether the synthesis confirmation is a formal checkpoint (emits event, stored in `planning_checkpoints` table) or just an inline prose confirmation
   - Recommendation: Emit `planning:checkpoint` for the synthesis confirmation AND store in `planning_checkpoints` for audit. This satisfies INTG-03 and CHKP requirements (though CHKP is Phase 8 — keep Phase 3 checkpoint simple).

---

## Sources

### Primary (HIGH confidence)

All findings are from direct codebase inspection:

- `infrastructure/runtime/src/dianoia/orchestrator.ts` — handle()/confirmResume()/abandon() methods, event emission pattern
- `infrastructure/runtime/src/dianoia/store.ts` — PlanningStore CRUD, updateProjectConfig() pattern
- `infrastructure/runtime/src/dianoia/types.ts` — PlanningProject interface, DianoiaState union
- `infrastructure/runtime/src/nous/pipeline/stages/context.ts` — system prompt injection pattern, planning block location
- `infrastructure/runtime/src/nous/working-state.ts` — extractWorkingState(), formatWorkingState()
- `infrastructure/runtime/src/pylon/server.ts` — createGateway(), modules array, RouteDeps construction
- `infrastructure/runtime/src/pylon/routes/plans.ts` — route module pattern (exact shape to replicate)
- `infrastructure/runtime/src/pylon/routes/deps.ts` — RouteDeps interface, extension point
- `infrastructure/runtime/src/koina/event-bus.ts` — EventName union (all 5 planning: events verified), emit() signature
- `infrastructure/runtime/src/organon/built-in/plan.ts` — plan_create tool, JSON output format
- `infrastructure/runtime/src/organon/built-in/plan-propose.ts` — plan_propose tool, __marker pattern, JSON-only output constraint
- `infrastructure/runtime/src/dianoia/index.ts` — current barrel exports
- `.planning/phases/02-orchestrator-and-entry/02-VERIFICATION.md` — verified Phase 2 wiring (getPlanningOrchestrator getter confirmed line 81-82 of manager.ts)

### Secondary (MEDIUM confidence)
- `.planning/research/SUMMARY.md` — ecosystem research (no new deps needed, zero-dependency stance confirmed)
- `.planning/phases/02-orchestrator-and-entry/02-01-SUMMARY.md` — key decisions from Phase 2 (sync methods, cast-through-unknown pattern)

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all from direct codebase inspection; zero new dependencies confirmed
- Architecture patterns: HIGH — plans.ts, server.ts, context.ts, orchestrator.ts all read directly
- Pitfalls: HIGH — plan_propose JSON marker pitfall verified from source code inspection; other pitfalls from logic analysis of existing patterns

**Research date:** 2026-02-24
**Valid until:** 2026-03-24 (stable codebase; plans.ts pattern unlikely to change before Phase 3 executes)
