# Phase 5: Requirements Definition - Research

**Researched:** 2026-02-24
**Domain:** Dianoia requirements scoping interaction — SQLite persistence, orchestrator pattern, FSM advancement
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Feature presentation format**
- One category at a time — agent presents a full category (table-stakes, then differentiators) and waits for scoping before moving on
- Table-stakes features presented first within each category, differentiators second
- Features sourced from research summary (Phase 4) + project context — not a hardcoded list
- Number of categories and features per category is determined by the project domain (no fixed count)

**Scoping interaction mechanics**
- Agent proposes v1/v2/out for each feature based on project goal, constraints, and research findings — intelligent proposals, not blank prompts
- User confirms or adjusts the proposals — much faster than deciding from scratch for every feature
- User can modify a previous category's decisions before advancing (until coverage gate is met)
- Freeform adjustments accepted: "move AUTH-02 to v2" or "make the first three v1" — agent interprets and updates

**REQ-ID format**
- Format: `CATEGORY-NUMBER` where category is a short uppercase abbreviation (2-6 chars)
- Category codes derived from the category name: AUTH, STOR, API, UI, NOTIF, INTG, etc.
- Numbering starts at 01 within each category
- Agent assigns IDs; user doesn't need to propose them
- IDs are stable once created — not reassigned if requirements change scope

**Testability standard**
- All requirements must be user-centric (describes observable behavior, not implementation)
- Agent enforces testability: if a proposed requirement is vague or implementation-specific, rephrase it before persisting
- Out-of-scope requirements include a rationale string: "Out of scope because: [reason]"

**Coverage validation gate**
- Must pass BOTH conditions before FSM fires `REQUIREMENTS_COMPLETE`:
  1. At least 1 v1 requirement exists
  2. All presented categories have a scoping decision (no category left undecided)
- Agent checks gate after each category is confirmed — announces when coverage is met
- User can still modify decisions after gate is met, then re-confirm to advance

### Claude's Discretion
- How many features to present per category (natural limit: ~5-8 per category is readable)
- Whether to present a summary of all scoped requirements before firing REQUIREMENTS_COMPLETE
- Exact wording of feature descriptions and proposals

### Deferred Ideas (OUT OF SCOPE)
- Requirement editing after roadmap is created — out of scope for Phase 5; would require roadmap regeneration
- User-defined custom categories — not needed for v1; agent derives categories from research
</user_constraints>

---

## Summary

Phase 5 adds the requirements scoping loop between the `requirements` FSM state (entered after research) and the `roadmap` state (entered when coverage gate passes). The pattern mirrors the Phase 3 questioning loop: methods on `DianoiaOrchestrator` drive the interaction (`presentCategory`, `processScopingDecision`, `updateRequirement`, `validateCoverage`, `completeRequirements`), a new tool (`plan_requirements`) handles the agent-facing execution, and a v23 schema migration adds the `rationale` column that out-of-scope requirements need.

The research synthesis from Phase 4 is the primary input. It is stored as a `planning_research` row with `dimension = 'synthesis'` and retrieved via `store.listResearch(projectId)` filtered to that dimension. The orchestrator reads this synthesis plus `project.projectContext` to derive feature categories and intelligent v1/v2/out proposals.

The `planning_requirements` table already exists (schema v20) with the correct `tier` CHECK constraint (`'v1'|'v2'|'out-of-scope'`). `PlanningStore.createRequirement()` and `listRequirements()` exist and are ready to use. The only gap is a missing `rationale` column (needed for REQS-06) and a missing `updateRequirement()` method for freeform adjustment.

**Primary recommendation:** Add v23 migration for `rationale`, add `updateRequirement()` to `PlanningStore`, then implement `RequirementsOrchestrator` (parallel to `ResearchOrchestrator`) and a `plan_requirements` tool that drives the agent-user scoping loop.

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| REQS-01 | Agent presents features by category with table-stakes vs. differentiator classification | `RequirementsOrchestrator.presentCategory()` reads synthesis row and derives feature list per category; table-stakes flag comes from research `features` dimension format |
| REQS-02 | User can scope each category (v1/v2/out of scope) through structured interaction | `plan_requirements` tool drives loop; `processScopingDecision()` writes requirements via `store.createRequirement()` per confirmed feature |
| REQS-03 | Requirements assigned REQ-IDs in `CATEGORY-NUMBER` format | `RequirementsOrchestrator` generates IDs: category derived from feature category name (uppercase, 2-6 chars), number auto-incremented per category |
| REQS-04 | Requirements persisted to `planning_requirements` table with tier | `store.createRequirement()` already handles this; `tier` CHECK constraint already in schema; no migration needed for this requirement |
| REQS-05 | Requirements are user-centric, specific, and testable | Enforced in `RequirementsOrchestrator.formatRequirement()` — vague descriptions rephrased before `createRequirement()` is called |
| REQS-06 | Out-of-scope requirements include rationale | Requires v23 migration: `ALTER TABLE planning_requirements ADD COLUMN rationale TEXT`; `createRequirement()` extended to accept optional `rationale` field |
| REQS-07 | Requirements coverage validated before advancing | `validateCoverage()` checks: (1) at least 1 v1 req exists, (2) all presented categories have at least one requirement; only then orchestrator fires `REQUIREMENTS_COMPLETE` |
</phase_requirements>

---

## Standard Stack

No new dependencies. Phase 5 uses the same primitives as Phases 3 and 4.

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| better-sqlite3 | 12.6.2 | Requirements persistence | Same db instance already wired |
| TypeScript | project version | All implementation | Project standard |
| vitest | project version | Unit tests | Project standard |

### Existing Infrastructure Used
| Component | Location | How Used |
|-----------|----------|----------|
| `PlanningStore` | `dianoia/store.ts` | `createRequirement()`, `listRequirements()`, `updateRequirement()` (to add) |
| `DianoiaOrchestrator` | `dianoia/orchestrator.ts` | Receives new requirements-phase methods |
| `transition()` | `dianoia/machine.ts` | `transition("requirements", "REQUIREMENTS_COMPLETE")` advances to roadmap |
| `eventBus` | `koina/event-bus.js` | `planning:phase-complete` on coverage gate passed |
| `createLogger("dianoia")` | `koina/logger.js` | Standard logging |
| `generateId("req")` | `koina/crypto.js` | Requirement DB row IDs (already used by `createRequirement`) |

**Installation: none required.**

---

## Architecture Patterns

### Recommended File Structure

```
dianoia/
  requirements.ts          # NEW: RequirementsOrchestrator (parallel to researcher.ts)
  requirements-tool.ts     # NEW: createPlanRequirementsTool (parallel to research-tool.ts)
  requirements.test.ts     # NEW: unit tests for RequirementsOrchestrator
  orchestrator.ts          # MODIFIED: presentCategory(), processScopingDecision(),
                           #           updateRequirement(), validateCoverage(), completeRequirements()
  store.ts                 # MODIFIED: updateRequirement(), rationale in createRequirement()
  schema.ts                # MODIFIED: PLANNING_V23_MIGRATION
  types.ts                 # MODIFIED: PlanningRequirement gets optional rationale field
  index.ts                 # MODIFIED: export RequirementsOrchestrator, createPlanRequirementsTool, PLANNING_V23_MIGRATION
```

### Pattern 1: Schema Migration (v23) for Rationale Column

The `planning_requirements` table needs a `rationale` column for out-of-scope requirements. Same pattern used for v21 and v22: export a constant from `schema.ts`, register it in `mneme/schema.ts` MIGRATIONS array.

```typescript
// dianoia/schema.ts — add after PLANNING_V22_MIGRATION
export const PLANNING_V23_MIGRATION = `ALTER TABLE planning_requirements ADD COLUMN rationale TEXT`;
```

```typescript
// mneme/schema.ts — add to MIGRATIONS array at version 23
import { PLANNING_V23_MIGRATION } from "../dianoia/schema.js";
// { version: 23, sql: PLANNING_V23_MIGRATION }
```

```typescript
// dianoia/types.ts — add rationale to PlanningRequirement
export interface PlanningRequirement {
  id: string;
  projectId: string;
  phaseId: string | null;
  reqId: string;
  description: string;
  category: string;
  tier: "v1" | "v2" | "out-of-scope";
  status: "pending" | "validated" | "skipped";
  rationale: string | null;  // ADD: only set for out-of-scope
  createdAt: string;
  updatedAt: string;
}
```

### Pattern 2: Store Method — updateRequirement

Missing from `PlanningStore`. Required for freeform adjustment ("move AUTH-02 to v2"). Pattern matches `updatePhaseStatus()`:

```typescript
// dianoia/store.ts — add after listRequirements()
updateRequirement(
  id: string,
  updates: { tier?: "v1" | "v2" | "out-of-scope"; rationale?: string | null },
): void {
  const update = this.db.transaction(() => {
    const sets: string[] = [];
    const vals: unknown[] = [];
    if (updates.tier !== undefined) { sets.push("tier = ?"); vals.push(updates.tier); }
    if (updates.rationale !== undefined) { sets.push("rationale = ?"); vals.push(updates.rationale); }
    if (sets.length === 0) return;
    sets.push("updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')");
    vals.push(id);
    const result = this.db
      .prepare(`UPDATE planning_requirements SET ${sets.join(", ")} WHERE id = ?`)
      .run(...vals);
    if (result.changes === 0) {
      throw new PlanningError(`Planning requirement not found: ${id}`, {
        code: "PLANNING_REQUIREMENT_NOT_FOUND",
        context: { id },
      });
    }
  });
  update();
}
```

Also extend `createRequirement()` to accept `rationale?: string | null` and pass it in the INSERT.

### Pattern 3: RequirementsOrchestrator Class

Parallel to `ResearchOrchestrator`. Takes `db` and reads research synthesis to drive category presentation.

```typescript
// dianoia/requirements.ts
import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import { PlanningStore } from "./store.js";
import { transition } from "./machine.js";

const log = createLogger("dianoia:requirements");

export interface FeatureProposal {
  name: string;
  description: string;
  isTableStakes: boolean;
  proposedTier: "v1" | "v2" | "out-of-scope";
  rationale?: string;
}

export interface CategoryProposal {
  category: string;           // e.g., "AUTH", "STOR"
  categoryName: string;       // e.g., "Authentication", "Data Storage"
  tableStakes: FeatureProposal[];
  differentiators: FeatureProposal[];
}

export class RequirementsOrchestrator {
  private store: PlanningStore;

  constructor(db: Database.Database) {
    this.store = new PlanningStore(db);
  }

  getSynthesis(projectId: string): string | null {
    const rows = this.store.listResearch(projectId);
    const synthesis = rows.find((r) => r.dimension === "synthesis");
    return synthesis?.content ?? null;
  }

  // Agent calls this to build the structured category list from synthesis
  // Returns prompt text for the LLM to display to user
  formatCategoryPresentation(category: CategoryProposal): string {
    const tsLines = category.tableStakes.map((f) =>
      `- **${f.name}**: ${f.description} → proposed: **${f.proposedTier}**`
    );
    const diffLines = category.differentiators.map((f) =>
      `- **${f.name}**: ${f.description} → proposed: **${f.proposedTier}**`
    );
    return [
      `## ${category.categoryName} (${category.category})`,
      "",
      "**Table stakes** (users expect these):",
      ...tsLines,
      "",
      "**Differentiators** (set products apart):",
      ...diffLines,
      "",
      "Confirm these proposals or adjust (e.g., 'move the second one to v2', 'make all v1'):",
    ].join("\n");
  }

  // Persist confirmed category decisions
  persistCategory(
    projectId: string,
    category: CategoryProposal,
    decisions: Array<{ name: string; tier: "v1" | "v2" | "out-of-scope"; rationale?: string }>,
  ): void {
    const existing = this.store.listRequirements(projectId)
      .filter((r) => r.category === category.category);
    const usedNumbers = existing.map((r) => {
      const match = r.reqId.match(/-(\d+)$/);
      return match ? parseInt(match[1]!, 10) : 0;
    });
    let nextNum = (usedNumbers.length > 0 ? Math.max(...usedNumbers) : 0) + 1;

    const allFeatures = [...category.tableStakes, ...category.differentiators];
    for (const decision of decisions) {
      const feature = allFeatures.find((f) => f.name === decision.name);
      if (!feature) continue;
      const reqId = `${category.category}-${String(nextNum).padStart(2, "0")}`;
      nextNum++;
      this.store.createRequirement({
        projectId,
        reqId,
        description: feature.description,
        category: category.category,
        tier: decision.tier,
        rationale: decision.tier === "out-of-scope" ? (decision.rationale ?? null) : null,
      });
    }
    log.info(`Persisted ${decisions.length} requirements for category ${category.category}`);
  }

  validateCoverage(projectId: string, presentedCategories: string[]): boolean {
    const reqs = this.store.listRequirements(projectId);
    const hasV1 = reqs.some((r) => r.tier === "v1");
    if (!hasV1) return false;
    for (const cat of presentedCategories) {
      const catReqs = reqs.filter((r) => r.category === cat);
      if (catReqs.length === 0) return false;
    }
    return true;
  }

  transitionToRoadmap(projectId: string): void {
    this.store.updateProjectState(projectId, transition("requirements", "REQUIREMENTS_COMPLETE"));
  }
}
```

### Pattern 4: plan_requirements Tool

Parallel to `research-tool.ts`. The agent calls this tool with structured JSON decisions; the tool persists them and checks coverage.

```typescript
// dianoia/requirements-tool.ts
export function createPlanRequirementsTool(
  orchestrator: DianoiaOrchestrator,
  requirementsOrchestrator: RequirementsOrchestrator,
): ToolHandler {
  return {
    definition: {
      name: "plan_requirements",
      description:
        "Persist scoped requirements for a category and check coverage gate. Call once per category with the user's final decisions. When coverage gate passes and user confirms, call with action='complete' to advance to roadmap phase.",
      input_schema: {
        type: "object",
        properties: {
          projectId: { type: "string" },
          action: { type: "string", enum: ["persist_category", "update_requirement", "check_coverage", "complete"] },
          category: { /* CategoryProposal shape */ },
          decisions: { type: "array", /* decision items */ },
          reqId: { type: "string", description: "For update_requirement action" },
          updates: { /* tier, rationale */ },
        },
        required: ["projectId", "action"],
      },
    },
    async execute(input, context): Promise<string> {
      // Routes to persistCategory, updateRequirement, validateCoverage, or transitionToRoadmap
    },
  };
}
```

### Pattern 5: Orchestrator Methods for Agent Loop

On `DianoiaOrchestrator`, add lightweight delegation methods (same pattern as `skipResearch()` and `completePhase()`):

```typescript
// In orchestrator.ts — new methods for requirements phase
getResearchSynthesis(projectId: string): string | null {
  // Delegates to requirementsOrchestrator or store.listResearch directly
}

completeRequirements(projectId: string, nousId: string, sessionId: string): string {
  this.store.updateProjectState(projectId, transition("requirements", "REQUIREMENTS_COMPLETE"));
  eventBus.emit("planning:phase-complete", { projectId, nousId, sessionId, phase: "requirements" });
  log.info(`Requirements complete for project ${projectId}; advancing to roadmap`);
  return "Requirements confirmed. Moving to roadmap generation.";
}
```

### Anti-Patterns to Avoid

- **Hardcoded category list**: Categories must come from the synthesis text, not a fixed enum. The research phase discovered the domain; trust those findings.
- **Storing categories as separate table**: The category is a `category` TEXT column on `planning_requirements` — no separate categories table needed.
- **FSM transition without coverage gate**: Call `validateCoverage()` before firing `REQUIREMENTS_COMPLETE`. The orchestrator, not the agent, is the authoritative gate.
- **Reassigning REQ-IDs after tier change**: `updateRequirement()` changes `tier` in place; the `req_id` is immutable once created (CONTEXT.md locked decision).
- **Calling `transition("requirements", "REQUIREMENTS_COMPLETE")` from tool layer**: Same pattern as `transitionToRequirements()` — the orchestrator/requirements orchestrator owns state transitions, the tool layer calls them.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| DB row for requirements | Custom JSON blob in project config | `store.createRequirement()` | Already exists; typed; queryable |
| REQ-ID uniqueness | In-memory counter | Query existing reqs per category + MAX | DB is source of truth across sessions |
| Category persistence | Separate categories table | `category` column on `planning_requirements` | Already in schema; listRequirements can group by category |
| FSM transition | Inline state write | `transition("requirements", "REQUIREMENTS_COMPLETE")` from machine.ts | Validates allowed transitions; throws on invalid event |
| Reading synthesis | Re-running research | `store.listResearch(projectId).find(r => r.dimension === 'synthesis')` | Synthesis row already persisted by Phase 4 |

**Key insight:** All the persistence primitives exist. Phase 5 is pure orchestration logic layered on top of the existing store.

---

## Common Pitfalls

### Pitfall 1: Missing v23 Migration in makeDb() Test Helper
**What goes wrong:** Tests that create in-memory DBs without running `PLANNING_V23_MIGRATION` will fail with "table has no column named rationale" when `createRequirement()` tries to insert rationale.
**Why it happens:** Every new migration must be added to all `makeDb()` helper functions in test files: `orchestrator.test.ts`, `store.test.ts`, `researcher.test.ts`.
**How to avoid:** Add `db.exec(PLANNING_V23_MIGRATION)` to every `makeDb()` in test files. Check all three existing test files.
**Warning signs:** `SqliteError: table planning_requirements has no column named rationale`

### Pitfall 2: PlanningRequirement mapRequirement Missing Rationale
**What goes wrong:** The `mapRequirement` private method in `store.ts` will return `rationale: undefined` (TypeScript error) if not updated after migration.
**Why it happens:** The mapper reads row columns explicitly; new columns not included will fail type check.
**How to avoid:** Update `mapRequirement` in `store.ts` to read `row["rationale"] as string | null` and add to returned object.
**Warning signs:** `npx tsc --noEmit` reports type error on PlanningRequirement shape.

### Pitfall 3: REQ-ID Counter Collision on Re-presented Category
**What goes wrong:** If the user goes back to modify a category, persisting the updated decisions creates duplicate REQ-IDs.
**Why it happens:** Naive counter starts at 01 for every call.
**How to avoid:** `persistCategory()` must query existing requirements for the category and find `MAX(number)` before numbering new ones. Use `updateRequirement()` for changes to existing requirements (not new INSERTs).
**Warning signs:** Duplicate `req_id` values in `planning_requirements` for same project.

### Pitfall 4: exactOptionalPropertyTypes Spread
**What goes wrong:** When merging optional fields into objects, direct assignment of potentially-undefined values fails type check.
**Why it happens:** TypeScript `exactOptionalPropertyTypes` is enabled in this project (learned in Phase 3).
**How to avoid:** Use conditional spread: `...(rationale !== undefined ? { rationale } : {})` in all merge operations.
**Warning signs:** `tsc --noEmit` reports "Type 'undefined' is not assignable to type 'string | null'"

### Pitfall 5: oxlint require-await on Async Tool execute()
**What goes wrong:** Tool `execute()` methods declared `async` that don't use `await` will fail oxlint `require-await`.
**Why it happens:** `plan_requirements` "persist" and "check" actions are synchronous; only "complete" involves async work.
**How to avoid:** Pattern from Phase 2: `return Promise.resolve(JSON.stringify(...))` for sync branches, OR use `await` in all branches. The research-tool.ts uses direct `async`/`await` — follow that pattern.
**Warning signs:** `npx oxlint src/` reports `require-await` violation.

### Pitfall 6: Coverage Gate on Skip Path
**What goes wrong:** If the user skips research, there's no synthesis to read. `getSynthesis()` returns null. The agent has no features to propose.
**Why it happens:** Research is optional (RESR-04). `skipResearch()` creates zero research rows.
**How to avoid:** When synthesis is null, `RequirementsOrchestrator` falls back to `project.projectContext` (constraints, keyDecisions) to derive a minimal feature list. The tool should surface this gracefully.
**Warning signs:** Agent presents "no features found" rather than deriving from project context.

---

## Code Examples

### Reading the Research Synthesis at Runtime

```typescript
// Source: codebase — researcher.ts synthesizeResearch() stores with dimension='synthesis'
const rows = store.listResearch(projectId);
const synthesis = rows.find((r) => r.dimension === "synthesis");
const content = synthesis?.content ?? null; // null when research was skipped
```

### FSM: requirements -> roadmap

```typescript
// Source: dianoia/machine.ts — TRANSITION_RESULT
// requirements: { REQUIREMENTS_COMPLETE: "roadmap", ABANDON: "abandoned" }
this.store.updateProjectState(projectId, transition("requirements", "REQUIREMENTS_COMPLETE"));
```

### createRequirement with rationale (after v23 migration)

```typescript
// Source: dianoia/store.ts createRequirement() — extended form
store.createRequirement({
  projectId: "proj-123",
  reqId: "AUTH-01",
  description: "User can log in with email and password",
  category: "AUTH",
  tier: "v1",
  rationale: null,  // null for v1/v2
});

store.createRequirement({
  projectId: "proj-123",
  reqId: "AUTH-03",
  description: "User can log in with SAML SSO",
  category: "AUTH",
  tier: "out-of-scope",
  rationale: "Out of scope because: enterprise-only feature, adds significant implementation complexity for v1",
});
```

### updateRequirement (new method, freeform adjustment)

```typescript
// Source: pattern from updatePhaseStatus() in store.ts
store.updateRequirement("req-row-id", { tier: "v2" });
store.updateRequirement("req-row-id", {
  tier: "out-of-scope",
  rationale: "Out of scope because: deferred to v2 to reduce launch scope",
});
```

### Coverage Validation

```typescript
// Source: CONTEXT.md locked decisions
function validateCoverage(projectId: string, presentedCategories: string[]): boolean {
  const reqs = store.listRequirements(projectId);
  const hasV1 = reqs.some((r) => r.tier === "v1");
  if (!hasV1) return false;
  return presentedCategories.every((cat) => reqs.some((r) => r.category === cat));
}
```

### test makeDb() Pattern (all three migrations)

```typescript
// Source: dianoia/researcher.test.ts makeDb()
import { PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION, PLANNING_V23_MIGRATION } from "./schema.js";

function makeDb(): Database.Database {
  const db = new Database(":memory:");
  db.exec(PLANNING_V20_DDL);
  db.exec(PLANNING_V21_MIGRATION);
  db.exec(PLANNING_V22_MIGRATION);
  db.exec(PLANNING_V23_MIGRATION);  // ADD in Phase 5
  return db;
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No requirements persistence | `planning_requirements` table with tier + status | Phase 1 (v20 DDL) | Table exists, `createRequirement()` and `listRequirements()` ready |
| No requirements update | Missing `updateRequirement()` | Gap — Phase 5 adds it | Freeform adjustment requires this |
| No rationale column | Missing — needs v23 migration | Gap — Phase 5 adds it | REQS-06 cannot be satisfied without it |
| Synthesis stored per-run | Single `dimension='synthesis'` row per project | Phase 4 | Readable with `listResearch().find(r => r.dimension === 'synthesis')` |

**Current gaps that Phase 5 must close:**
- `PLANNING_V23_MIGRATION`: rationale column on planning_requirements
- `PlanningStore.updateRequirement()`: update tier + rationale in place
- `RequirementsOrchestrator`: category presentation + persistence logic
- `createPlanRequirementsTool`: agent-facing tool
- `orchestrator.ts`: `completeRequirements()` wrapper method
- Export all new items from `dianoia/index.ts`

---

## Open Questions

1. **How does the agent parse freeform adjustment text?**
   - What we know: CONTEXT.md says freeform adjustments are accepted ("move AUTH-02 to v2")
   - What's unclear: Is parsing done in the tool execute() or in the orchestrator?
   - Recommendation: Parse in the tool `execute()` body (same as how the tool layer builds user-facing messages in Phase 4). The tool receives `reqId` + `updates` fields after the agent interprets the user's natural language. The LLM does the NL parsing; the tool just persists the structured result. No custom NL parser needed.

2. **Does `plan_requirements` need to store "in-progress" category state?**
   - What we know: User can modify previous categories; all decisions are in `planning_requirements` table; category is a queryable column
   - What's unclear: Whether a separate "presented categories" list needs to be persisted to SQLite or can live as a tool call artifact
   - Recommendation: Store presented categories in `project.config` JSON as `presentedCategories: string[]` (same cast-through-unknown pattern as `pendingConfirmation` in Phase 2). This survives session restart. The coverage gate needs this list.

3. **What happens when the research synthesis is absent (skip path)?**
   - What we know: `skipResearch()` creates zero research rows; `getSynthesis()` returns null
   - What's unclear: Whether requirements phase should block or have a fallback
   - Recommendation: Fallback to `project.projectContext` (constraints, keyDecisions) to derive a minimal feature list. The agent should surface this to the user: "No research was performed. I'll derive feature categories from your project context instead."

---

## Sources

### Primary (HIGH confidence)
- Codebase: `infrastructure/runtime/src/dianoia/schema.ts` — confirmed `planning_requirements` DDL, tier CHECK constraint, no rationale column
- Codebase: `infrastructure/runtime/src/dianoia/store.ts` — confirmed `createRequirement()`, `listRequirements()`, no `updateRequirement()`; `mapRequirement()` does not map rationale
- Codebase: `infrastructure/runtime/src/dianoia/machine.ts` — confirmed `requirements` state, `REQUIREMENTS_COMPLETE` event, transition to `roadmap`
- Codebase: `infrastructure/runtime/src/dianoia/orchestrator.ts` — confirmed no requirements-phase methods exist; `skipResearch()` and `completePhase()` patterns to follow
- Codebase: `infrastructure/runtime/src/dianoia/researcher.ts` — confirmed synthesis stored as `dimension='synthesis'`; `ResearchOrchestrator` class pattern to mirror
- Codebase: `infrastructure/runtime/src/dianoia/types.ts` — confirmed `PlanningRequirement` interface; rationale field absent
- Codebase: `.planning/phases/03-project-context-and-api/03-01-SUMMARY.md` — confirmed questioning loop pattern; `exactOptionalPropertyTypes` gotcha
- Codebase: `.planning/phases/04-research-pipeline/04-02-SUMMARY.md` — confirmed synthesis row pattern; `trySafeAsync` signature warning

### Secondary (MEDIUM confidence)
- `.planning/STATE.md` decisions log — confirms project patterns: `pendingConfirmation` in config JSON, `cast-through-unknown`, sync methods preferred over async when no await

### Tertiary (LOW confidence)
- None

---

## Metadata

**Confidence breakdown:**
- Schema gaps (v23 migration, missing updateRequirement): HIGH — verified by direct code inspection
- Architecture (RequirementsOrchestrator pattern): HIGH — mirrors ResearchOrchestrator which is proven
- FSM transitions: HIGH — machine.ts read directly; REQUIREMENTS_COMPLETE -> roadmap confirmed
- Synthesis retrieval: HIGH — researcher.ts stores with dimension='synthesis'; listResearch() confirmed
- Pitfalls (exactOptionalPropertyTypes, oxlint require-await, REQ-ID collision): HIGH — all occurred in Phases 2-4 and documented in STATE.md

**Research date:** 2026-02-24
**Valid until:** 2026-03-24 (stable — all findings from codebase direct inspection, not external docs)
