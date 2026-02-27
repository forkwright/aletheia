# Spec 32 — Dianoia v2: Context-Engineered Planning with Sub-Agent Isolation

| Field   | Value                                                  |
|---------|--------------------------------------------------------|
| Status  | Implemented (Phases 1–3 complete, Phase 4 verification complete; learning/retrospective deferred to Spec 42) |
| Author  | Syn                                                    |
| Created | 2026-02-24                                             |
| Scope   | `infrastructure/runtime/src/dianoia/`, `ui/src/`, agent workflows |
| Spec    | 32                                                     |
| Depends | Spec 31 (current Dianoia), Spec 31 (context engineering) |

---

## Problem

Dianoia v1 (Spec 31) established the foundation: SQLite persistence, a finite state machine, wave-based execution, and goal-backward verification. But operating it exposed five structural problems that can't be patched incrementally:

### 1. The orchestrator drowns in its own context

The orchestrating agent (Syn) calls every planning tool directly. Research results, requirements categorization, roadmap generation, execution monitoring — all of it accumulates in the orchestrator's context window. By the time execution starts, 100k+ tokens of mechanical work have degraded the orchestrator's judgment. This is the exact problem Cody identified: **"the 1st token is better than the 100,000th."**

### 2. State doesn't survive distillation

SQLite persists structured data, but the orchestrator's *understanding* of the project — the why, the trade-offs, the decisions — lives only in the chat context. When distillation compresses the session, this understanding is lossy-encoded into a paragraph. Requirements confirmed in detail are reduced to bullet summaries. Discussion context vanishes entirely. The project is technically alive in SQLite but practically dead in the agent's mind.

### 3. No discussion before phases

Dianoia v1 has a questioning phase at project creation but nothing equivalent before individual phases. Phase planning jumps straight from requirements to execution plans. Gray areas, ambiguities, and preference decisions that should surface *per phase* are either guessed by the planner or discovered mid-execution — too late to course-correct cheaply.

### 4. No sub-agent isolation

Every tool call runs in the orchestrator's context. Research spawns sub-agents but pipes their output back into the orchestrator. Requirements, roadmap, and execution are all orchestrator-direct. This violates the principle that the orchestrator should hold strategy while sub-agents hold execution detail.

### 5. No planning UI

Planning happens entirely through chat. There's no visual distinction between planning mode and normal conversation. No structured question interface. No project visualization. No way to see at a glance where a project stands, what's been decided, or what's next.

---

## Design Principles

1. **Orchestrator stays clean.** The orchestrating agent holds PROJECT.md + ROADMAP.md + current phase status. Everything else is delegated. Target: orchestrator context stays under 40k tokens regardless of project complexity.

2. **Every execution unit gets fresh context.** Sub-agents receive a scoped context packet built from project files, not inherited from chat history. Each starts at token 1 with exactly the information needed for its task.

3. **Files are the source of truth.** Markdown files in a project directory are human-readable, agent-readable, and survive everything — restarts, distillation, session switches, even database corruption. SQLite is the index; files are the record.

4. **Discussion is a first-class phase step.** Before any phase plans or executes, gray areas are surfaced and resolved. Decisions are captured and become constraints.

5. **The UI signals mode.** Planning mode should feel different from chat. Structured inputs for structured questions. Visual project state always visible.

6. **Model fits task.** Haiku for exploration and test-running. Sonnet for implementation. Opus for architecture and strategic decisions. Cost follows complexity.

---

## Architecture

### Project Directory Structure

Every project gets a directory under the orchestrating agent's workspace:

```
.dianoia/projects/{project-id}/
├── PROJECT.md            # Name, scope, status, key decisions, context
├── REQUIREMENTS.md       # Confirmed requirements with tiers and rationale
├── ROADMAP.md            # Phase sequence, dependencies, milestone markers
├── RESEARCH.md           # Synthesized research findings
└── phases/
    └── {phase-id}/
        ├── DISCUSS.md    # Gray areas identified, questions asked, decisions captured
        ├── PLAN.md       # Execution plan — steps, waves, model assignments
        ├── STATE.md      # Live execution state — wave progress, sub-agent status
        └── VERIFY.md     # Verification results, gap analysis, remediation notes
```

**File generation rules:**
- `PROJECT.md` — written at project creation, updated at each state transition
- `REQUIREMENTS.md` — written when requirements phase completes
- `ROADMAP.md` — written when roadmap is committed
- `RESEARCH.md` — written when research synthesis completes
- Phase files — written at their respective phase steps

**SQLite relationship:** The `planning_projects`, `planning_phases`, `planning_requirements` tables remain as the structured index. A new `project_dir` column on `planning_projects` points to the file directory. On any read, the orchestrator can reconstruct context from files alone — SQLite is for queries (list projects, filter by state, find phase by ID), not for context injection.

### State Machine Changes

The v1 FSM adds a `discussing` state between `phase-planning` and `executing`:

```
idle → questioning → researching → requirements → roadmap
  → [per phase: discussing → planning → executing → verifying]
    → [next phase or complete]
```

New states:
- **`discussing`** — Phase-level gray area identification and resolution
- **`planning`** (renamed from `phase-planning`) — Execution plan generation with model/role assignments

New transitions:
```
roadmap       --ROADMAP_COMPLETE-->   discussing
discussing    --DISCUSSION_COMPLETE--> planning
planning      --PLAN_READY-->          executing
verifying     --NEXT_PHASE-->          discussing   (not back to planning)
```

The `discussing → planning → executing → verifying → discussing` loop is the per-phase cycle. Each iteration targets the next phase in `phase_order`.

### Orchestrator Architecture

```
┌─────────────────────────────────────────┐
│           DianoiaOrchestrator v2         │
│                                         │
│  Holds: PROJECT.md, ROADMAP.md,         │
│         current phase summary           │
│  Does:  Strategic synthesis,            │
│         phase sequencing decisions,     │
│         sub-agent dispatch,             │
│         progress monitoring,            │
│         human communication             │
│  Doesn't: Research, requirements        │
│           categorization, planning      │
│           detail, code execution,       │
│           verification analysis         │
│                                         │
│  ┌───────────────────────────────────┐  │
│  │      Context Packet Builder       │  │
│  │                                   │  │
│  │  Reads project files + codebase   │  │
│  │  Filters to phase-relevant scope  │  │
│  │  Assembles scoped context for     │  │
│  │  each sub-agent spawn             │  │
│  └───────────────────────────────────┘  │
│                                         │
│  ┌───────────────────────────────────┐  │
│  │      Sub-Agent Dispatcher         │  │
│  │                                   │  │
│  │  Spawns with role + model + ctx   │  │
│  │  Monitors completion/failure      │  │
│  │  Collects structured results      │  │
│  │  Updates STATE.md per wave        │  │
│  └───────────────────────────────────┘  │
└─────────────────────────────────────────┘
         │                    │
    ┌────┴────┐          ┌───┴────┐
    │ Sub-    │   ...    │ Sub-   │
    │ Agent 1 │          │ Agent N│
    │ (fresh) │          │ (fresh)│
    └─────────┘          └────────┘
```

### Context Packet Specification

Each sub-agent spawn receives a context packet — a structured prompt prefix assembled from project files. The packet varies by task type:

| Task Type | Included Files | Model | Max Context Budget |
|-----------|---------------|-------|-------------------|
| Research (per dimension) | PROJECT.md, dimension prompt | Sonnet | 8k |
| Research synthesis | PROJECT.md, all dimension outputs | Sonnet | 16k |
| Requirements analysis | PROJECT.md, RESEARCH.md, category prompt | Sonnet | 12k |
| Phase discussion analysis | PROJECT.md, REQUIREMENTS.md (filtered), ROADMAP.md, phase context | Opus | 16k |
| Phase planning | PROJECT.md, phase DISCUSS.md, requirements (filtered), codebase context | Sonnet | 24k |
| Execution (per step) | PLAN.md (this step), DISCUSS.md decisions, relevant codebase files | Sonnet/Haiku | 32k |
| Verification | Phase goal, success criteria, PLAN.md, execution output summaries | Sonnet | 16k |

**Codebase context** for execution steps is assembled by the Context Packet Builder:
1. Read the step's file targets from PLAN.md
2. Include those files (or relevant excerpts)
3. Include any shared types/interfaces referenced
4. Include test files if the step involves testing
5. Hard cap at budget — truncate least-relevant files first

### Discuss-Per-Phase Flow

When a phase enters `discussing` state:

1. **Gray Area Identification** — A sub-agent (Opus) receives:
   - The phase goal and requirements
   - Decisions from previous phases (if any)
   - Known constraints from PROJECT.md
   - The current state of relevant code (if applicable)
   
   It produces a structured list of gray areas: ambiguities, trade-off points, dependency risks, preference questions.

2. **Structured Question Presentation** — The orchestrator presents each gray area to the user as a structured question:
   - Description of the ambiguity
   - 2-4 proposed options with rationale for each
   - A free-text option for answers that don't fit the proposals
   - Recommended option (if the agent has a clear preference)

3. **Decision Capture** — User selections are written to `phases/{id}/DISCUSS.md` with:
   - The question as asked
   - The selected option or free-text response
   - Any additional user notes
   - Timestamp

4. **Constraint Propagation** — Decisions from DISCUSS.md become hard constraints in the phase PLAN.md. The planning sub-agent receives them as "decided — do not re-litigate."

### Wave Execution Model

Phase plans decompose into steps with explicit dependency edges. The execution orchestrator computes waves:

```
Wave 0: [step-A, step-B]        ← no dependencies, run parallel
Wave 1: [step-C]                ← depends on A
Wave 2: [step-D, step-E]        ← D depends on C, E depends on B
```

Each step in a wave is a separate sub-agent spawn. The orchestrator:
- Dispatches all steps in the current wave
- Monitors for completion (poll STATE.md or receive structured results)
- On wave completion: updates STATE.md, advances to next wave
- On step failure: strategic decision — retry (once), skip (if non-critical), or escalate to human

### Verification Protocol

Goal-backward verification after each phase:

1. **Verification sub-agent** receives:
   - Phase goal and success criteria (from ROADMAP.md)
   - Phase requirements (from REQUIREMENTS.md, filtered)
   - Phase decisions (from DISCUSS.md)
   - Current codebase state (relevant files)
   - Execution summary (from STATE.md)

2. **Gap analysis** — For each success criterion:
   - `met` — criterion is satisfied with evidence
   - `partially-met` — partially satisfied, specific gap identified
   - `not-met` — not satisfied, root cause identified

3. **Result** → `VERIFY.md`:
   - Overall status: met / partially-met / not-met
   - Per-criterion assessment
   - Gap list with proposed remediation
   - Recommendation: advance / remediate / escalate

4. **Orchestrator decision:**
   - All met → advance to next phase (or complete)
   - Gaps exist → present to human with remediation options
   - Critical gaps → block project, require human input

---

## UI Design

### Planning Mode Activation

Planning mode activates when a Dianoia project is active (state ≠ idle, complete, abandoned). The UI shifts to reflect a different interaction model.

**Visual indicators:**
- Chat header shows project name + current phase
- Background or border subtle color shift (planning accent color)
- Planning panel appears above chat input

### Planning Panel

A persistent, compact panel above the chat text input area. Same theme as the rest of the UI. Contains:

**Milestone Timeline** (default view):
```
┌──────────────────────────────────────────────────┐
│  ● Research  ● Requirements  ◉ Phase 1  ○ Phase 2  ○ Verify │
│  ─────────────────────●────────────────────────── │
│  Phase 1: State Machine + File Persistence        │
│  Status: Discussing (2/5 decisions made)          │
└──────────────────────────────────────────────────┘
```

- Filled circles (●) = complete
- Current circle (◉) = in progress, highlighted
- Empty circles (○) = pending
- No dates — milestones only (as specified: "those are all terrible guesses anyway")
- Current phase name and status shown below the timeline
- Hover on any milestone → tooltip with: phase name, goal, requirement count, status, blockers

**Expandable detail** (click a phase):
- Phase goal
- Requirements assigned to this phase
- Discussion decisions (if any)
- Execution wave progress (if executing)
- Verification results (if verified)

**Visualization variants** (project-dependent, selectable):
- **Timeline** — default, good for sequential phases
- **Dependency graph** — when phases have cross-dependencies
- **Kanban columns** — when phases are largely independent

### Decision Cards

During discuss phases, structured questions render as cards, not chat bubbles:

```
┌─────────────────────────────────────────────┐
│  🔶 How should the file watcher handle      │
│     concurrent writes?                       │
│                                              │
│  ○ Debounce (500ms window, last write wins)  │
│    └ Simple, handles most cases              │
│                                              │
│  ○ Queue (FIFO, process sequentially)        │
│    └ Preserves ordering, slower              │
│                                              │
│  ○ Lock file (mutex, fail fast on conflict)  │
│    └ Safest, requires retry logic            │
│                                              │
│  ○ Custom: ___________________________       │
│                                              │
│  Recommended: Debounce                       │
│                                              │
│           [Confirm]  [Skip]                  │
└─────────────────────────────────────────────┘
```

- Radio selection for options
- Free-text field always available
- Optional "Add note" text area for context
- Confirm persists decision to DISCUSS.md
- Skip marks the question as deferred (agent uses its recommendation)

### Progress During Execution

During wave execution, the planning panel updates to show:
- Current wave number / total waves
- Per-step status (pending / running / done / failed)
- Expandable log summary per step (last 3 lines of output)
- Estimated completion based on step count (not time)

### API Endpoints (New)

```
GET  /api/planning/projects/:id/discuss     — Current discussion questions
POST /api/planning/projects/:id/discuss     — Submit decision for a question
GET  /api/planning/projects/:id/timeline    — Milestone data for planning panel
GET  /api/planning/projects/:id/phases/:pid/detail — Expanded phase detail
WS   /api/planning/projects/:id/stream      — Real-time execution updates
```

---

## Implementation Phases

### Phase 1: File-Backed State + Discussion Loop

**Goal:** Projects survive everything. Discussion before phases becomes real.

- Create `.dianoia/projects/` directory structure and file generators
- Add `project_dir` column to `planning_projects`
- Write PROJECT.md / REQUIREMENTS.md / ROADMAP.md / RESEARCH.md at appropriate state transitions
- Add `discussing` state to FSM with transitions
- Build gray area identification prompt and discussion capture flow
- Write DISCUSS.md at discussion completion
- Propagate discussion decisions as constraints to planning step
- Update `DianoiaOrchestrator.handle()` to read from files, not just SQLite

**Success criteria:**
- Project can be reconstructed from files alone (delete SQLite rows, re-import from .dianoia/)
- Discussion questions surface before phase planning
- Decisions persist in DISCUSS.md and constrain subsequent planning

### Phase 2: Sub-Agent Context Engineering

**Goal:** Orchestrator context stays clean. Sub-agents get exactly what they need.

- Build `ContextPacketBuilder` — reads project files, filters to task scope, assembles prompt prefix
- Define context budgets per task type (see table above)
- Refactor research phase: orchestrator dispatches, doesn't consume results directly
- Refactor requirements phase: sub-agent categorizes, orchestrator reviews and confirms with human
- Refactor phase planning: sub-agent generates PLAN.md, orchestrator validates structure
- Refactor execution: each step is a sub-agent with scoped context packet
- Add model selection per task type (Haiku/Sonnet/Opus routing)
- Orchestrator reads summaries and STATE.md, not raw execution output

**Success criteria:**
- Orchestrator context stays under 40k tokens through a multi-phase project
- Each sub-agent receives <32k tokens of relevant context
- Model selection routes appropriately (verify via spawn logs)

### Phase 3: Planning UI

**Goal:** Planning mode feels different. Visual project state. Structured inputs.

- Planning panel component (milestone timeline, hover tooltips, expandable detail)
- Decision card component (radio options, free text, confirm/skip)
- Planning mode header (project name, phase, accent color)
- `/api/planning/.../timeline` and `/api/planning/.../discuss` endpoints
- WebSocket stream for execution progress updates
- Visualization variant selector (timeline / dependency graph / kanban)

**Success criteria:**
- Planning panel renders with accurate milestone state
- Decision cards accept input and persist to DISCUSS.md via API
- Execution progress updates in real time via WebSocket
- Visual distinction between planning mode and normal chat is obvious

### Phase 4: Verification + Learning

**Goal:** Phases are verified against goals. Projects teach future projects.

- ✅ Goal-backward verification sub-agent with scoped context (`verifier.ts`, uses `buildContextPacketSync`)
- ✅ VERIFY.md generation with per-criterion gap analysis (`project-files.ts`)
- ✅ Gap remediation flow (remediation wave or human escalation)
- ❌ Cross-project skill extraction — **deferred to Spec 42 (Nous Team, Gap 1: closed feedback loops)**
- ❌ Project retrospective generation — **deferred to Spec 42 (Nous Team, reflection→action gap)**
- ❌ Completed projects → reusable insights — **deferred to Spec 42 (Nous Team, competence model)**

**Success criteria:**
- ✅ Verification catches genuine gaps (not just rubber-stamp approval)
- ✅ Gap remediation produces targeted fixes
- ❌ Completed projects generate reusable insights — deferred to Spec 42

**Rationale for deferral:** The learning items (skill extraction, retrospectives, reusable insights) are the core thesis of Spec 42 — connecting existing primitives (competence model, reflection, dianoia) into closed feedback loops. Implementing them here would split the concern. Verification is complete; learning belongs in the spec that owns the feedback loop architecture.

---

## Migration Strategy

Dianoia v1 is live with the existing tool surface (`plan_create`, `plan_research`, `plan_requirements`, `plan_roadmap`, `plan_execute`, `plan_verify`). Migration approach:

1. **Phase 1 is additive.** File generation layers on top of existing SQLite writes. No tools change signature. Existing projects gain file backing.

2. **Phase 2 refactors internals.** Tool handlers call the same API but the orchestrator delegates differently. The tool surface is stable; the execution model changes underneath.

3. **Phase 3 adds UI.** New API endpoints. New components. No tool changes needed — the UI reads from the same SQLite + file state.

4. **Phase 4 extends.** Verification improvements. New capabilities. No breaking changes.

At no point does the existing tool surface break. Projects created with v1 continue to work. The transition is incremental — each phase delivers independently useful capability.

---

## What This Doesn't Cover

- **Multi-user planning.** Single orchestrator per project. No collaborative editing.
- **Cross-project dependencies.** Projects are independent. Phase 4's learning is retrospective, not live coordination.
- **Mobile UI.** Planning panel is desktop-first. Mobile gets a simplified status view.
- **External integrations.** No Jira/Linear/GitHub Issues sync. Projects live in Aletheia.

---

## References

- Spec 31 — Dianoia v1 (current implementation)
- Spec 31 — Context Engineering (cache-aware bootstrap, skill manifest, turn cost classifier)
- GSD (Claude Code) — discuss-phase pattern, file-backed state, fresh context per execution unit
- Claude Code plan mode — structured question UI, mode shift UX
