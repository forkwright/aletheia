# Phase 4: Research Pipeline - Research

**Researched:** 2026-02-24
**Domain:** Parallel sub-agent orchestration, sessions_dispatch, ephemeralSoul, SQLite research storage, FSM researching state
**Confidence:** HIGH (all findings sourced directly from codebase)

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

- Fixed 4 dimensions: stack, features, architecture, pitfalls — always the same 4, predictable and testable
- Each dimension has its own ephemeralSoul definition (specific instructions per dimension)
- Dimension-specific soul definitions are in code, not configurable by user at runtime
- Skip behavior: orchestrator offers skip inline; user can accept or skip inline (not just a flag at invocation time)
- When skipped: FSM transitions questioning -> requirements directly (bypasses researching entirely)
- No research records created when skipped; planning proceeds from project context only
- Timeout behavior: capture completed dimensions, mark stalled dimension as `partial` status in planning_research
- Synthesizer runs with whatever completed (3/4 or 2/4 dimensions is fine)
- User is told which dimension timed out: "Architecture research timed out — synthesized from 3 of 4 dimensions"
- Does NOT fail whole research phase; best-effort always preferred
- Synthesis output has fixed sections: Stack, Features, Architecture, Pitfalls, Recommendations
- Stored: one row per dimension in planning_research + synthesized summary in project record

### Claude's Discretion

- Exact timeout threshold value (recommend 90-120 seconds per researcher)
- How ephemeralSoul definitions are structured (inline in orchestrator vs separate soul files)
- Whether synthesis is produced by a dedicated 5th agent or by the orchestrator inline
- How partial dimension results are surfaced to the user (inline message vs status block)

### Deferred Ideas (OUT OF SCOPE)

- Dynamic dimension selection based on project type
- User-configurable dimension list

</user_constraints>

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| RESR-01 | Agent can spawn 4 parallel domain researchers (stack, features, architecture, pitfalls) via sessions_dispatch | sessions_dispatch supports up to 10 parallel tasks via Promise.allSettled; 4 fits comfortably |
| RESR-02 | Research results stored in planning_research table per-dimension | PlanningStore.createResearch() already exists; schema has id/project_id/phase/dimension/content/created_at |
| RESR-03 | Synthesizer agent produces consolidated research summary after all researchers complete | sessions_dispatch returns all results (including partial/timeout) synchronously via Promise.allSettled; synthesizer runs after |
| RESR-04 | Research can be skipped (user already knows the domain) | FSM has RESEARCH_COMPLETE event on researching state; skip path transitions directly questioning→researching→requirements via START_RESEARCH then RESEARCH_COMPLETE |
| RESR-05 | Researcher subagents use ephemeralSoul definitions specific to each research dimension | sessions_spawn supports ephemeral=true with ephemeralSoul string; BUT sessions_dispatch does NOT — dispatchers use role presets only |
| RESR-06 | Research phase has timeout; partial results captured if one researcher stalls | sessions_dispatch has per-task timeoutSeconds field; stalled tasks return status:"timeout"; Promise.allSettled ensures all tasks resolve |

</phase_requirements>

---

## Summary

Phase 4 adds `ResearchOrchestrator` — a new class in `dianoia/researcher.ts` — that drives the project from `researching` state to `requirements` state. When the FSM enters `researching` (via `confirmSynthesis` in Phase 3), the orchestrator needs a mechanism to spawn 4 parallel domain researchers, collect their results, store each per-dimension in `planning_research`, synthesize them, and fire `RESEARCH_COMPLETE` to advance the FSM.

The critical discovery is that `sessions_dispatch` (the parallel dispatcher) does NOT support `ephemeralSoul` — that parameter is exclusive to `sessions_spawn` which caps at 3 concurrent tasks. For 4 parallel researchers, the implementation must use `sessions_dispatch` with the `researcher` role preset, embedding the dimension-specific soul instructions in the `context` field of each task (prepended to the task description). This is how the dispatcher passes per-task context — the `context` field is prepended verbatim to the task message. This avoids both the 3-agent limit and the ephemeralSoul mismatch while still giving each researcher dimension-specific instructions.

The skip path requires two FSM transitions: `START_RESEARCH` (already fired by `confirmSynthesis`) puts the project in `researching`, then `RESEARCH_COMPLETE` advances it to `requirements`. When the user skips research, the orchestrator fires `RESEARCH_COMPLETE` immediately without spawning any agents or writing any `planning_research` rows. The synthesizer (whether inline or a 5th agent) produces the synthesis text which is stored as a special "synthesis" row in `planning_research` (dimension="synthesis") or in `planning_projects.project_context`. Given that `planning_research` already has a `dimension` column, storing synthesis as `dimension="synthesis"` is the cleanest approach — no schema migration needed.

**Primary recommendation:** Use `sessions_dispatch` with `researcher` role and embed dimension-specific soul definitions in the `context` field per task. Store each result via `PlanningStore.createResearch()` and add a `synthesizeResearch()` method to `DianoiaOrchestrator`. The synthesizer can run inline (orchestrator method) for simplicity, or as a 5th `sessions_spawn` call for quality. Either fits the codebase.

---

## Standard Stack

### Core (all already in the project — no new dependencies)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `sessions_dispatch` | existing | Spawn 4 parallel researchers, collect all results | Handles up to 10 tasks, per-task timeout, Promise.allSettled partial results |
| `PlanningStore.createResearch()` | existing | Store per-dimension research results | Already implemented in store.ts; schema has correct columns |
| `DianoiaOrchestrator` | existing | Drive FSM transitions, coordinate research | All orchestration methods already wired; add `runResearch()` here |
| `vitest` | existing | Unit tests for ResearchOrchestrator | makeDb() + in-memory SQLite pattern already established in orchestrator.test.ts |
| `better-sqlite3` | 12.6.2 | SQLite persistence for planning_research rows | Already wired; PLANNING_V20_DDL created planning_research table |

### sessions_dispatch Parameter Reference (HIGH confidence — sourced from sessions-dispatch.ts)

Each task in the `tasks` array accepts:

```typescript
interface DispatchTask {
  role?: "coder" | "reviewer" | "researcher" | "explorer" | "runner";
  task: string;          // Primary task description
  context?: string;      // Prepended verbatim to task message — use this for soul injection
  agentId?: string;      // Which nous to run as (default: caller's nous)
  model?: string;        // Model override
  timeoutSeconds?: number; // Per-task timeout in seconds (default: 180)
}
```

Return shape from `sessions_dispatch`:

```typescript
{
  taskCount: number;
  succeeded: number;
  failed: number;
  results: Array<{
    index: number;
    role?: string;
    task: string;
    status: "success" | "error" | "timeout";
    result?: string;      // Sub-agent response text
    structuredResult?: SubAgentResult; // Parsed from ```json block
    error?: string;
    tokens?: { input, output, total };
    durationMs: number;
  }>;
  timing: { wallClockMs, sequentialMs, savedMs };
  totalTokens: number;
}
```

**Critical:** `sessions_dispatch` uses `Promise.allSettled` — a single task timing out does NOT block others. The tool's framework timeout is set to 0 (no framework timeout) because the tool manages its own per-task timeouts.

### Why NOT sessions_spawn for Parallel Research

`sessions_spawn` supports `ephemeralSoul` but caps parallel tasks at `MAX_PARALLEL = 3` (hardcoded in `sessions-spawn.ts` line 385). Requesting 4 tasks silently drops the 4th. Use `sessions_dispatch` instead and pass soul content via the `context` field.

---

## Architecture Patterns

### Recommended Module Structure Addition

```
dianoia/
  researcher.ts        — NEW: ResearchOrchestrator (or research methods on DianoiaOrchestrator)
  researcher.test.ts   — NEW: unit tests for research pipeline
  orchestrator.ts      — MODIFIED: add runResearch(), skipResearch(), synthesizeResearch()
  store.ts             — UNCHANGED: createResearch() and listResearch() already exist
```

The architecture choice: either add research logic directly to `DianoiaOrchestrator` (simpler, consistent with existing pattern) or create a `researcher.ts` module. Given the CONTEXT.md calls for a `ResearchOrchestrator`, create a separate class in `researcher.ts` that is injected into or composed with `DianoiaOrchestrator`.

### Pattern 1: ResearchOrchestrator Class

**What:** A class that encapsulates research pipeline logic — spawning, collecting, storing, synthesizing.
**When to use:** Phase 4 plan 04-01.

```typescript
// Source: /home/ckickertz/summus/aletheia/infrastructure/runtime/src/dianoia/researcher.ts (to create)
import type { AgentDispatcher } from "../organon/built-in/sessions-spawn.js";
import type { PlanningStore } from "./store.js";

export class ResearchOrchestrator {
  constructor(
    private store: PlanningStore,
    private dispatcher: AgentDispatcher,
  ) {}

  async runResearch(projectId: string, projectGoal: string, timeoutSeconds = 90): Promise<void> {
    // 1. Dispatch 4 parallel researchers via sessions_dispatch
    // 2. Collect results (all complete or timeout)
    // 3. Store per-dimension results via store.createResearch()
    // 4. Run synthesizer (inline or 5th spawn)
    // 5. Store synthesis row
  }

  skipResearch(projectId: string): void {
    // No-op for research storage; caller transitions FSM
  }
}
```

### Pattern 2: Dimension Task Definition

Each research dimension is a task object with dimension-specific soul injected via `context`:

```typescript
// Source: sessions-dispatch.ts DispatchTask interface (verified)
const DIMENSIONS = ["stack", "features", "architecture", "pitfalls"] as const;
type ResearchDimension = typeof DIMENSIONS[number];

function buildResearchTask(
  dimension: ResearchDimension,
  projectGoal: string,
  timeoutSeconds: number,
): DispatchTask {
  return {
    role: "researcher",
    task: `Research the ${dimension} dimension for this project: ${projectGoal}`,
    context: DIMENSION_SOULS[dimension],   // Injected verbatim before task
    timeoutSeconds,
  };
}
```

### Pattern 3: Result Collection and Partial Handling

```typescript
// Source: sessions-dispatch.ts lines 263-308 (verified)
// sessions_dispatch returns after ALL tasks resolve (success, error, or timeout)
// The CONTEXT.md decision: mark stalled dimensions as "partial" status
// planning_research table has no status column — store "partial" in content field or add column

// Option A: Encode partial in content (no schema migration)
if (result.status === "timeout") {
  store.createResearch({
    projectId,
    phase: "research",
    dimension,
    content: JSON.stringify({ status: "partial", reason: "timeout", partial: result.error }),
  });
}

// Option B: Add status column to planning_research (migration v22)
// ALTER TABLE planning_research ADD COLUMN status TEXT NOT NULL DEFAULT 'complete'
//   CHECK(status IN ('complete', 'partial', 'failed'))
```

**Recommendation for planner:** Option B (migration v22 adding status column) is cleaner and matches CONTEXT.md language ("mark stalled dimension as `partial` status"). Migration v21 is already established as the pattern.

### Pattern 4: FSM Transitions for Skip Path

```typescript
// Source: machine.ts VALID_TRANSITIONS (verified)
// researching state accepts: RESEARCH_COMPLETE, BLOCK, ABANDON
// Skip path: project is already in "researching" (confirmSynthesis put it there)
// Fire RESEARCH_COMPLETE immediately without spawning any agents

function skipResearch(projectId: string): void {
  // No createResearch() calls
  store.updateProjectState(projectId, transition("researching", "RESEARCH_COMPLETE"));
  // State is now "requirements" — Phase 5 starts
}
```

**Critical:** The FSM transition from `researching` → `requirements` uses `RESEARCH_COMPLETE`. This already exists in `TRANSITION_RESULT` (machine.ts line 38). No FSM changes needed.

### Pattern 5: Synthesizer (Inline vs 5th Agent)

Two valid approaches per CONTEXT.md discretion:

**Inline synthesis** (orchestrator method): Concatenate dimension results and call the LLM via `sessions_spawn` with `researcher` role. Simpler, no extra dispatch layer.

**5th agent synthesis**: Use `sessions_spawn` (single task) with `researcher` role after dispatch completes. Clean separation, allows synthesis to do multi-step web research if needed.

Either stores result as `dimension="synthesis"` in `planning_research` table. The planner chooses.

### Anti-Patterns to Avoid

- **Using sessions_spawn tasks array for 4 researchers:** The `tasks` array in sessions_spawn is capped at `MAX_PARALLEL = 3`. The 4th task is silently dropped. Always use `sessions_dispatch` for 4 parallel tasks.
- **Using ephemeral=true in sessions_dispatch:** `sessions_dispatch` does not support `ephemeral` or `ephemeralSoul` parameters. Soul injection must go through the `context` field.
- **Calling dispatcher directly from orchestrator:** The `ResearchOrchestrator` needs a `dispatcher: AgentDispatcher` — the same dispatcher type used by `createSessionsDispatchTool`. This is injected at construction time, not imported from a module.
- **Forgetting the `planning_research` status column:** The current schema has no `status` column. The CONTEXT.md decision to mark dimensions as "partial" requires either a schema migration or encoding status in content JSON. Pick one and be consistent.
- **Transitioning FSM inside ResearchOrchestrator:** Keep FSM transitions in `DianoiaOrchestrator`, not in `ResearchOrchestrator`. Separation of concerns matches the existing pattern.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Parallel task dispatch with timeout | Custom Promise.race wrappers | `sessions_dispatch` tool | Already handles per-task timeout, partial results, audit trail, token counting |
| Waiting for all 4 tasks (even if some fail) | Custom Promise.allSettled loop | `sessions_dispatch` (internally uses Promise.allSettled) | Partial results already surfaced in results array with status field |
| Per-researcher timeout | Custom timer per agent | `timeoutSeconds` field on each DispatchTask | Built into sessions_dispatch; task returns `status: "timeout"` not a thrown error |
| Research result storage | Custom DB write | `PlanningStore.createResearch()` | Already implemented; `listResearch(projectId)` fetches all for synthesis |

**Key insight:** The entire parallel dispatch and result collection infrastructure already exists. Phase 4 is primarily glue code: build task definitions, call sessions_dispatch, read results, write to planning_research, run synthesizer.

---

## Common Pitfalls

### Pitfall 1: sessions_spawn 3-Task Cap Silently Drops the 4th Researcher

**What goes wrong:** Using `sessions_spawn` with `tasks: [stack, features, architecture, pitfalls]` — the 4th task (pitfalls) is silently dropped because `MAX_PARALLEL = 3` in `sessions-spawn.ts`. No error thrown.
**Why it happens:** sessions_spawn's parallel path uses `capped = tasks.slice(0, MAX_PARALLEL)` with a warn log, not an error return.
**How to avoid:** Always use `sessions_dispatch` (separate tool, different implementation) for 4+ parallel tasks. sessions_dispatch allows up to 10.
**Warning signs:** Only 3 `planning_research` rows created; pitfalls dimension always missing.

### Pitfall 2: ephemeralSoul is Not a sessions_dispatch Parameter

**What goes wrong:** Passing `ephemeralSoul` in a sessions_dispatch task definition — it silently has no effect. Each researcher gets no dimension-specific identity.
**Why it happens:** `ephemeralSoul` is only parsed in `sessions-spawn.ts` ephemeral path, not in `sessions-dispatch.ts`.
**How to avoid:** Embed soul content in the `context` field of each DispatchTask. The dispatcher prepends `context` verbatim before `task` in the message: `${context}\n\n---\n\n${task}`.
**Warning signs:** All 4 researchers return generic research; no dimension-specific framing in results.

### Pitfall 3: FSM State Confusion — Skip Needs Two Transitions

**What goes wrong:** Assuming skip means "don't call confirmSynthesis" — but `confirmSynthesis` already transitioned the project to `researching`. The skip path must fire `RESEARCH_COMPLETE` from `researching` to reach `requirements`.
**Why it happens:** The FSM is driven by transitions, not state assignments. A project in `researching` cannot jump to `requirements` without `RESEARCH_COMPLETE`.
**How to avoid:** Skip path: `transition("researching", "RESEARCH_COMPLETE")`. This is already in `TRANSITION_RESULT`. No FSM changes.
**Warning signs:** `updateProjectState` throws `PLANNING_INVALID_TRANSITION` error on skip.

### Pitfall 4: planning_research Has No status Column

**What goes wrong:** Trying to mark a row as "partial" via a status field that doesn't exist on the table.
**Why it happens:** `PLANNING_V20_DDL` defines planning_research with only `id, project_id, phase, dimension, content, created_at`. No status column.
**How to avoid:** Add `PLANNING_V22_MIGRATION` (`ALTER TABLE planning_research ADD COLUMN status TEXT NOT NULL DEFAULT 'complete' CHECK(status IN ('complete', 'partial', 'failed'))`). Follow the migration pattern from v21 — export constant from schema.ts, import into mneme/schema.ts MIGRATIONS array.
**Warning signs:** TypeScript compiler error on `store.createResearch({ ..., status: "partial" })`; or DB constraint violation if attempting raw SQL insert.

### Pitfall 5: Synthesizer Context Window — 4 Full Research Results

**What goes wrong:** Passing all 4 dimension results verbatim to a synthesizer agent overflows context or produces low-quality synthesis because the synthesizer has too much raw text.
**Why it happens:** Each researcher may return 2000-5000 tokens of text. 4 × 4000 = 16000 tokens of context before the synthesizer even starts its system prompt.
**How to avoid:** Truncate each result to a reasonable length (e.g. 1500 chars) before passing to synthesizer. The `parseStructuredResult` function extracts `summary` and `details` from a JSON block if the researcher returns structured output. Use `structuredResult.summary` (short) rather than `result` (full text) when available.
**Warning signs:** Synthesizer agent hits token budget; synthesis output is fragmented or cuts off.

### Pitfall 6: oxlint require-await on async Methods with No Await

**What goes wrong:** Declaring `async runResearch()` without an `await` inside — oxlint flags `require-await`.
**Why it happens:** oxlint enforces that `async` functions must contain at least one `await`. This happened in Phase 2 (handle() was initially async).
**How to avoid:** Ensure `runResearch()` contains `await dispatcher.handleMessage(...)` or equivalent. If a helper method has no await path, declare it sync. The dispatcher call is `await`-able, so this is naturally satisfied.
**Warning signs:** `npx oxlint src/dianoia/researcher.ts` reports `require-await` warning.

---

## Code Examples

Verified patterns from actual codebase:

### sessions_dispatch call pattern (4 tasks)

```typescript
// Source: infrastructure/runtime/src/organon/built-in/sessions-dispatch.ts (verified)
// The tool is called via dispatcher.handleMessage — ResearchOrchestrator calls it indirectly
// To use sessions_dispatch from ResearchOrchestrator, call dispatcher.handleMessage with the
// formatted task OR call the tool execute() directly if tool context is available

// Direct execute() pattern (if ToolContext is available):
const dispatchTool = createSessionsDispatchTool(dispatcher);
const rawResult = await dispatchTool.execute({
  tasks: [
    {
      role: "researcher",
      task: `Research the STACK dimension for: ${projectGoal}\n\nReturn your findings in a structured ```json block with status, summary, details, confidence fields.`,
      context: STACK_SOUL,       // Dimension-specific soul injected here
      timeoutSeconds: 90,
    },
    {
      role: "researcher",
      task: `Research the FEATURES dimension for: ${projectGoal}\n\nReturn your findings in a structured ```json block.`,
      context: FEATURES_SOUL,
      timeoutSeconds: 90,
    },
    // ... architecture, pitfalls
  ],
}, toolContext);
const dispatchResult = JSON.parse(rawResult);
// dispatchResult.results[i].status === "success" | "error" | "timeout"
// dispatchResult.results[i].result = full text from researcher
// dispatchResult.results[i].structuredResult = parsed JSON block (if present)
```

### PlanningStore.createResearch() (already implemented)

```typescript
// Source: infrastructure/runtime/src/dianoia/store.ts lines 334-353 (verified)
store.createResearch({
  projectId: "proj_xxx",
  phase: "research",          // Always "research" for this phase
  dimension: "stack",         // One of: stack, features, architecture, pitfalls, synthesis
  content: "## Stack\n...",   // Raw text or JSON string from researcher
});

// Retrieve all dimensions for a project:
const allResearch = store.listResearch(projectId);
// Returns PlanningResearch[]: { id, projectId, phase, dimension, content, createdAt }
```

### FSM Transition for Research Complete

```typescript
// Source: infrastructure/runtime/src/dianoia/machine.ts (verified)
// researching → requirements via RESEARCH_COMPLETE
import { transition } from "./machine.js";
store.updateProjectState(projectId, transition("researching", "RESEARCH_COMPLETE"));
// Next state: "requirements"
```

### Migration v22 Pattern (following v21 established pattern)

```typescript
// Source: infrastructure/runtime/src/dianoia/schema.ts (verified pattern)
// Add to schema.ts:
export const PLANNING_V22_MIGRATION = `
  ALTER TABLE planning_research ADD COLUMN status TEXT NOT NULL DEFAULT 'complete'
    CHECK(status IN ('complete', 'partial', 'failed'))
`;

// Add to mneme/schema.ts MIGRATIONS array (following v21 pattern):
// import { PLANNING_V22_MIGRATION } from "../dianoia/schema.js";
// { version: 22, sql: PLANNING_V22_MIGRATION }
```

### makeDb() Test Helper Pattern (for researcher.test.ts)

```typescript
// Source: infrastructure/runtime/src/dianoia/orchestrator.test.ts lines 17-22 (verified)
function makeDb(): Database.Database {
  const db = new Database(":memory:");
  db.exec(PLANNING_V20_DDL);
  db.exec(PLANNING_V21_MIGRATION);
  db.exec(PLANNING_V22_MIGRATION); // Add this for Phase 4 tests
  return db;
}
```

---

## Critical Architecture Decisions for Planner

### Decision 1: sessions_dispatch needs a ToolContext

`sessions_dispatch` is a tool handler, not a standalone function. Its `execute()` requires a `ToolContext` (`{ nousId, sessionId, workspace }`). The `ResearchOrchestrator` must receive either:

- A `ToolContext` at call time (passed from the agent pipeline), OR
- A pre-constructed `createSessionsDispatchTool(dispatcher)` instance with the context provided at execute time

The existing pattern (from Phase 2) is that `DianoiaOrchestrator` receives a `db` at construction. For `ResearchOrchestrator`, the `dispatcher` needs the same injection. Look at how `createRuntime()` in `aletheia.ts` wires the `AgentDispatcher` — that's the injection point.

### Decision 2: Where Does runResearch() Get Called?

After `confirmSynthesis()` transitions the project to `researching`, something must trigger `runResearch()`. Options:

1. **Agent-driven**: The agent receives "Context saved. Moving to research phase." from `confirmSynthesis()`, then the agent calls `sessions_dispatch` directly or uses a new `plan_research` tool. Agent-driven is more flexible but requires a tool.
2. **Orchestrator-driven**: `confirmSynthesis()` itself fires `runResearch()` after transitioning state. Simpler but makes `confirmSynthesis()` async and long-running.
3. **Pipeline-driven**: A new tool (`plan_research`) that the agent calls from the research phase.

The existing pattern favors option 1 or 3 (agent calls tools). The CONTEXT.md says the orchestrator "offers skip inline when announcing research" — this implies the agent is presenting the skip offer, which means the agent is the driver. Plan for a `plan_research` tool or an explicit method the agent calls via the tool pipeline.

### Decision 3: Synthesis Storage — planning_research vs project_context

Two options for storing the synthesis result:
- `planning_research` with `dimension = "synthesis"` (one more row, consistent storage)
- `planning_projects.project_context` JSON field (extend ProjectContext with `researchSummary`)

Recommendation: `planning_research` with `dimension = "synthesis"`. Keeps all research in one table, consistent with how Phase 5 reads research (via `listResearch(projectId)`). Phase 5 consumers need only query `planning_research` to get all context.

---

## State of the Art

| Old Approach | Current Approach | Impact |
|--------------|------------------|--------|
| Ephemeral agents via sessions_spawn for parallel research | sessions_dispatch with `context` field for soul injection | Removes 3-agent cap, enables true 4-way parallelism |
| Manual Promise.allSettled wrappers | sessions_dispatch built-in partial results | Timeout handling, audit trail, token counting included free |
| Research stored in markdown files | planning_research SQLite table | Survives session restart; queryable by project |

---

## Open Questions

1. **Where does runResearch() get called in the agent pipeline?**
   - What we know: `confirmSynthesis()` transitions to `researching` and returns a string the agent sees
   - What's unclear: Is there a `plan_research` tool, or does the agent call sessions_dispatch directly?
   - Recommendation: Add a `plan_research` tool to `dianoia/tools.ts` that calls `ResearchOrchestrator.runResearch()`. Consistent with Phase 2 pattern (plan_project tool).

2. **Does ResearchOrchestrator need a ToolContext, or does it call dispatcher.handleMessage directly?**
   - What we know: sessions_dispatch.execute() needs a ToolContext; dispatcher.handleMessage() needs InboundMessage
   - What's unclear: Which interface does ResearchOrchestrator use?
   - Recommendation: ResearchOrchestrator takes `AgentDispatcher` + constructs its own minimal ToolContext from the nousId/sessionId parameters it receives. This mirrors how sessions-dispatch.ts itself constructs `sessionKey` from context.

3. **Status column in planning_research: migration v22 or encode in content?**
   - What we know: Current schema has no status column; CONTEXT.md says "mark stalled dimension as `partial` status"
   - What's unclear: Is a schema migration appropriate here or is encoding in content simpler?
   - Recommendation: Migration v22. The planner established that migrations are the right pattern (v20, v21 precedent). A `status` column makes it queryable and avoids JSON parsing to determine partial state.

---

## Sources

### Primary (HIGH confidence)

- `/home/ckickertz/summus/aletheia/infrastructure/runtime/src/organon/built-in/sessions-dispatch.ts` — full implementation, DispatchTask/DispatchResult interfaces, maxItems:10, Promise.allSettled behavior, timeout handling
- `/home/ckickertz/summus/aletheia/infrastructure/runtime/src/organon/built-in/sessions-spawn.ts` — MAX_PARALLEL=3 confirmed, ephemeralSoul parameter confirmed (spawn only), tasks array parallel cap
- `/home/ckickertz/summus/aletheia/infrastructure/runtime/src/dianoia/schema.ts` — planning_research DDL confirmed (no status column), PLANNING_V21_MIGRATION pattern confirmed
- `/home/ckickertz/summus/aletheia/infrastructure/runtime/src/dianoia/store.ts` — createResearch() and listResearch() already implemented
- `/home/ckickertz/summus/aletheia/infrastructure/runtime/src/dianoia/machine.ts` — RESEARCH_COMPLETE event confirmed, researching → requirements transition confirmed
- `/home/ckickertz/summus/aletheia/infrastructure/runtime/src/dianoia/orchestrator.ts` — confirmSynthesis() confirmed as entry point to researching state
- `/home/ckickertz/summus/aletheia/infrastructure/runtime/src/organon/parallel.ts` — sessions_dispatch marked "never" for parallel grouping (long-running, blocks until all complete)
- `/home/ckickertz/summus/aletheia/infrastructure/runtime/src/organon/timeout.ts` — sessions_dispatch framework timeout = 0 (manages its own)
- `/home/ckickertz/summus/aletheia/infrastructure/runtime/src/nous/roles/index.ts` — researcher role: claude-sonnet-4, web_search+web_fetch+read+exec tools, maxTurns=10

---

## Metadata

**Confidence breakdown:**
- sessions_dispatch behavior: HIGH — read full implementation + tests
- ephemeralSoul limitation: HIGH — confirmed MAX_PARALLEL=3 in sessions-spawn.ts; ephemeralSoul not in sessions-dispatch.ts
- FSM transitions: HIGH — RESEARCH_COMPLETE confirmed in machine.ts TRANSITION_RESULT
- planning_research schema: HIGH — read DDL; no status column confirmed
- PlanningStore.createResearch(): HIGH — fully implemented, tested
- Synthesis storage location: MEDIUM — recommendation based on pattern; planner should decide
- Tool context injection: MEDIUM — inference from existing wiring patterns; exact wiring TBD in plan

**Research date:** 2026-02-24
**Valid until:** 2026-03-24 (stable TypeScript codebase)
