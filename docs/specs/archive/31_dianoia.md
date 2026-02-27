# Spec 31 — Dianoia: Persistent Multi-Phase Planning Runtime

| Field   | Value                                      |
|---------|--------------------------------------------|
| Status  | Implemented (Phase 2 entry points missing — see gaps below) |
| Author  | Demiurge                                   |
| Created | 2026-02-24                                 |
| Scope   | `infrastructure/runtime/src/dianoia/`      |
| Spec    | 31                                         |

---

## Problem

Aletheia's original planning tools (`plan_create`, `plan_propose`) operate within a single session. When the session ends, all planning state is lost. Starting a new session means restarting from scratch: re-explaining the goal, re-deriving requirements, re-generating plans. There is no way to pause mid-project and resume later, no mechanism to track which phases have been executed, and no way to coordinate multiple subagents working in parallel on a shared project.

Concrete gaps:

- **No persistence.** Planning objects live only in session memory. A runtime restart or session expiry destroys them.
- **No state machine.** Nothing enforces which planning stages are valid next steps. The agent can jump from questioning to execution without research or requirements.
- **No multi-agent coordination.** Parallel subagent work has no shared bookkeeping. Two agents can't independently execute different phases of the same project.
- **No human-in-loop gates.** High-risk decisions (irreversible deployments, schema migrations) proceed without a checkpoint mechanism.
- **No verification.** Phases complete without any goal-backward check that what was built actually achieves the phase goal.

Dianoia addresses all five gaps. It replaces the session-scoped tools with a persistent planning runtime backed by SQLite, driven by a finite state machine, and exposing a tool surface that subagents use to advance project state.

---

## Design

### Principles

- **Persistence over re-derivation.** All planning state lives in SQLite. Restarting the runtime, ending a session, or swapping agents does not lose project state.
- **State machine as the single source of truth.** Every state change goes through `DianoiaOrchestrator`, which delegates to `transition()` in `machine.ts`. No direct state writes bypass the FSM.
- **Constructor injection.** Every orchestrator class (`DianoiaOrchestrator`, `ResearchOrchestrator`, `RequirementsOrchestrator`, `RoadmapOrchestrator`, `ExecutionOrchestrator`, `GoalBackwardVerifier`, `CheckpointSystem`) accepts `db` and `dispatchTool` as constructor arguments. There is no global state.
- **Async-safe subagent execution.** Phase execution spawns subagents via `sessions_dispatch`. The FSM coordinates sequencing; the execution orchestrator manages wave-based parallelism within a phase. The planning FSM itself is sync.

### Architecture

The Dianoia module sits inside `infrastructure/runtime/src/dianoia/` and consists of these layers:

```
tools (plan_research, plan_requirements, plan_roadmap, plan_execute, plan_verify)
      |
orchestrators (DianoiaOrchestrator, ResearchOrchestrator, RequirementsOrchestrator,
               RoadmapOrchestrator, ExecutionOrchestrator, GoalBackwardVerifier, CheckpointSystem)
      |
store (PlanningStore — thin SQLite adapter, no business logic)
      |
schema (PLANNING_V20_DDL … PLANNING_V25_MIGRATION — migration constants)
      |
machine (transition(), VALID_TRANSITIONS — pure FSM, no I/O)
```

The `DianoiaOrchestrator` is the primary entry point. It is instantiated in `createRuntime()` and stored on `NousManager` via a getter/setter pair, making it accessible to API routes and tool handlers without being added to `AletheiaRuntime`'s public interface.

The full project lifecycle is a finite state machine with 11 states and 15 transitions. See the [State Machine](#state-machine) section for the complete diagram.

---

## SQLite Schema

All planning tables are created by the `dianoia` migration set. Each migration builds on the previous; they are applied in order by the existing `PlanningStore.migrate()` call.

### V20 (base schema)

```sql
CREATE TABLE IF NOT EXISTS planning_projects (
  id TEXT PRIMARY KEY,
  nous_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  goal TEXT NOT NULL,
  state TEXT NOT NULL DEFAULT 'idle' CHECK(state IN ('idle', 'questioning', 'researching', 'requirements', 'roadmap', 'phase-planning', 'executing', 'verifying', 'complete', 'blocked', 'abandoned')),
  config TEXT NOT NULL DEFAULT '{}',
  context_hash TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_planning_projects_nous ON planning_projects(nous_id);

CREATE TABLE IF NOT EXISTS planning_phases (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES planning_projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  goal TEXT NOT NULL,
  requirements TEXT NOT NULL DEFAULT '[]',
  success_criteria TEXT NOT NULL DEFAULT '[]',
  plan TEXT,
  status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'executing', 'complete', 'failed', 'skipped')),
  phase_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_planning_phases_project ON planning_phases(project_id, phase_order);

CREATE TABLE IF NOT EXISTS planning_requirements (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES planning_projects(id) ON DELETE CASCADE,
  phase_id TEXT,
  req_id TEXT NOT NULL,
  description TEXT NOT NULL,
  category TEXT NOT NULL,
  tier TEXT NOT NULL DEFAULT 'v1' CHECK(tier IN ('v1', 'v2', 'out-of-scope')),
  status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'validated', 'skipped')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_planning_requirements_project ON planning_requirements(project_id);

CREATE TABLE IF NOT EXISTS planning_checkpoints (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES planning_projects(id) ON DELETE CASCADE,
  type TEXT NOT NULL,
  question TEXT NOT NULL,
  decision TEXT,
  context TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_planning_checkpoints_project ON planning_checkpoints(project_id);

CREATE TABLE IF NOT EXISTS planning_research (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES planning_projects(id) ON DELETE CASCADE,
  phase TEXT NOT NULL,
  dimension TEXT NOT NULL,
  content TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_planning_research_project ON planning_research(project_id);
```

### V21

Adds optional project context (synthesized from the questioning phase):

```sql
ALTER TABLE planning_projects ADD COLUMN project_context TEXT
```

### V22

Adds research quality tracking:

```sql
ALTER TABLE planning_research ADD COLUMN status TEXT NOT NULL DEFAULT 'complete'
  CHECK(status IN ('complete', 'partial', 'failed'))
```

### V23

Adds rationale for out-of-scope requirements:

```sql
ALTER TABLE planning_requirements ADD COLUMN rationale TEXT
```

### V24

Adds spawn records for wave-based execution tracking (survive restart, enable zombie detection):

```sql
CREATE TABLE IF NOT EXISTS planning_spawn_records (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES planning_projects(id) ON DELETE CASCADE,
  phase_id TEXT NOT NULL REFERENCES planning_phases(id) ON DELETE CASCADE,
  wave_number INTEGER NOT NULL,
  session_key TEXT,
  status TEXT NOT NULL DEFAULT 'pending'
    CHECK(status IN ('pending', 'running', 'done', 'failed', 'skipped', 'zombie')),
  error_message TEXT,
  partial_output TEXT,
  started_at TEXT,
  completed_at TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_spawn_records_project
  ON planning_spawn_records(project_id, wave_number);
CREATE INDEX IF NOT EXISTS idx_spawn_records_phase
  ON planning_spawn_records(phase_id, status);
```

### V25

Adds risk-based checkpoint fields and phase verification result:

```sql
ALTER TABLE planning_checkpoints ADD COLUMN risk_level TEXT NOT NULL DEFAULT 'low'
  CHECK(risk_level IN ('low', 'medium', 'high'));
ALTER TABLE planning_checkpoints ADD COLUMN auto_approved INTEGER NOT NULL DEFAULT 0;
ALTER TABLE planning_checkpoints ADD COLUMN user_note TEXT;
ALTER TABLE planning_phases ADD COLUMN verification_result TEXT;
```

---

## State Machine

The planning project FSM has 11 states. `complete` and `abandoned` are terminal (no outgoing transitions). All non-terminal states except `complete` accept the `ABANDON` event.

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

<!--
ASCII fallback (for editors without Mermaid support):
idle --START_QUESTIONING--> questioning --START_RESEARCH--> researching
researching --RESEARCH_COMPLETE--> requirements --REQUIREMENTS_COMPLETE--> roadmap
roadmap --ROADMAP_COMPLETE--> phase-planning --PLAN_READY--> executing
executing --VERIFY--> verifying --NEXT_PHASE--> phase-planning
                                --ALL_PHASES_COMPLETE--> complete
                                --PHASE_FAILED--> blocked
executing --BLOCK--> blocked --RESUME--> executing
[any active state] --ABANDON--> abandoned
-->

### State Semantics

| State | Meaning |
|-------|---------|
| `idle` | Project created; no questioning started yet |
| `questioning` | Agent is gathering project context from the user |
| `researching` | Parallel researcher subagents running (stack, features, architecture, pitfalls) |
| `requirements` | User and agent collaboratively scoping requirements by category |
| `roadmap` | Roadmap being generated from requirements; user reviewing phases |
| `phase-planning` | Individual phase plans being produced and checker-validated |
| `executing` | Wave-based subagent execution in progress |
| `verifying` | Goal-backward verification agent running for the completed phase |
| `blocked` | Execution halted by a failed phase, verification failure, or pause flag |
| `complete` | All phases executed and verified; project done |
| `abandoned` | Project explicitly abandoned by user or agent |

### TypeScript Types

```typescript
export type DianoiaState =
  | "idle"
  | "questioning"
  | "researching"
  | "requirements"
  | "roadmap"
  | "phase-planning"
  | "executing"
  | "verifying"
  | "complete"
  | "blocked"
  | "abandoned";

export type PlanningEvent =
  | "START_QUESTIONING"
  | "START_RESEARCH"
  | "RESEARCH_COMPLETE"
  | "REQUIREMENTS_COMPLETE"
  | "ROADMAP_COMPLETE"
  | "PLAN_READY"
  | "VERIFY"
  | "NEXT_PHASE"
  | "ALL_PHASES_COMPLETE"
  | "PHASE_FAILED"
  | "BLOCK"
  | "RESUME"
  | "ABANDON";
```

---

## API Surface

All planning routes are mounted under `/api/planning/`. They are registered in `src/dianoia/routes.ts` and wired into the Pylon server via `planningRoutes(deps, refs)`.

**Common error responses:**

| Status | Body | Meaning |
|--------|------|---------|
| 503 | `{ "error": "Planning not enabled" }` | `planningOrchestrator` not available in `RouteDeps` |
| 503 | `{ "error": "Execution orchestrator not available" }` | `executionOrchestrator` not available |
| 404 | `{ "error": "Project not found" }` | No project with the given `:id` |

---

### 1. List Projects

```
GET /api/planning/projects
```

Returns a summary list of all planning projects across all nous instances. No path parameters.

**Response 200:**

```json
[
  {
    "id": "proj_abc123",
    "goal": "Build a distributed task queue",
    "state": "executing",
    "createdAt": "2026-02-24T10:00:00.000Z",
    "updatedAt": "2026-02-24T12:34:56.789Z"
  }
]
```

Summary fields only. Full project detail (config, context, contextHash, nousId) is returned only by route 2.

---

### 2. Get Project

```
GET /api/planning/projects/:id
```

Returns the full project snapshot for a single project.

**Path parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `id` | string | Project ID (e.g. `proj_abc123`) |

**Response 200:**

```json
{
  "id": "proj_abc123",
  "nousId": "nous_xyz",
  "sessionId": "sess_uvw",
  "goal": "Build a distributed task queue",
  "state": "executing",
  "config": {
    "depth": "comprehensive",
    "parallelization": true,
    "research": true,
    "plan_check": true,
    "verifier": true,
    "mode": "yolo"
  },
  "projectContext": {
    "goal": "Build a distributed task queue",
    "coreValue": "Reliability under load",
    "constraints": ["TypeScript only", "must use existing SQLite store"],
    "keyDecisions": ["Redis-free", "No external queue service"],
    "rawTranscript": [{ "turn": 1, "text": "I want to build..." }]
  },
  "contextHash": "a3f9b1c2d4e5f6a7",
  "createdAt": "2026-02-24T10:00:00.000Z",
  "updatedAt": "2026-02-24T12:34:56.789Z"
}
```

**TypeScript interface:**

```typescript
interface PlanningProject {
  id: string;
  nousId: string;
  sessionId: string;
  goal: string;
  state: DianoiaState;
  config: PlanningConfig;
  contextHash: string;
  createdAt: string;
  updatedAt: string;
  projectContext: ProjectContext | null;
}

interface ProjectContext {
  goal?: string;
  coreValue?: string;
  constraints?: string[];
  keyDecisions?: string[];
  rawTranscript?: Array<{ turn: number; text: string }>;
}
```

---

### 3. Get Roadmap

```
GET /api/planning/projects/:id/roadmap
```

Returns the project's phase list with execution status for each phase. Used to display the roadmap after generation and to track progress during execution.

**Path parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `id` | string | Project ID |

**Response 200:**

```json
{
  "projectId": "proj_abc123",
  "state": "executing",
  "phases": [
    {
      "id": "phase_001",
      "name": "Foundation",
      "goal": "Set up SQLite schema and state machine",
      "requirements": ["FOUND-01", "FOUND-02"],
      "successCriteria": ["Schema creates without error", "FSM rejects invalid transitions"],
      "phaseOrder": 0,
      "status": "complete",
      "hasPlan": true
    }
  ]
}
```

`hasPlan` is `true` if a phase plan object is stored (`plan` column is non-null). The plan contents are not returned here; use the phase plan directly if needed.

---

### 4. Get Execution Snapshot

```
GET /api/planning/projects/:id/execution
```

Returns the current execution state for a project, including the status of every spawn record and the active wave number.

**Path parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `id` | string | Project ID |

**Response 200:**

```json
{
  "projectId": "proj_abc123",
  "state": "executing",
  "activeWave": 1,
  "plans": [
    {
      "phaseId": "phase_001",
      "name": "Foundation",
      "status": "done",
      "waveNumber": 0,
      "startedAt": "2026-02-24T12:00:00.000Z",
      "completedAt": "2026-02-24T12:05:00.000Z",
      "error": null
    }
  ],
  "activePlanIds": ["phase_002"],
  "startedAt": "2026-02-24T12:00:00.000Z",
  "completedAt": null
}
```

**TypeScript interfaces:**

```typescript
interface ExecutionSnapshot {
  projectId: string;
  state: string;
  activeWave: number | null;
  plans: PlanEntry[];
  activePlanIds: string[];
  startedAt: string | null;
  completedAt: string | null;
}

interface PlanEntry {
  phaseId: string;
  name: string;
  status: string;  // pending | running | done | failed | skipped | zombie
  waveNumber: number | null;
  startedAt: string | null;
  completedAt: string | null;
  error: string | null;
}
```

`activeWave` is `null` when no plans are currently running. `completedAt` on the snapshot is `null` until all spawn records have a terminal status (`done`, `failed`, `skipped`, or `zombie`).

---

### 5. Get Phase Status

```
GET /api/planning/projects/:id/phases/:phaseId/status
```

Returns the execution status for a single phase within a project, filtered from the full execution snapshot. Useful for the UI status pill when focusing on a specific phase.

**Path parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `id` | string | Project ID |
| `phaseId` | string | Phase ID |

**Response 200:**

```json
{
  "phaseId": "phase_002",
  "projectId": "proj_abc123",
  "status": "executing",
  "waveCount": 2,
  "currentWave": 1,
  "plans": [
    {
      "phaseId": "phase_002",
      "name": "Orchestrator & Entry",
      "status": "running",
      "waveNumber": 1,
      "startedAt": "2026-02-24T12:10:00.000Z",
      "completedAt": null,
      "error": null
    }
  ]
}
```

`waveCount` is the total number of waves computed for this phase (`max(waveNumber) + 1` across all plans). `currentWave` is the `activeWave` from the full execution snapshot (not phase-scoped).

**TypeScript interface:**

```typescript
interface PhasePlanStatus {
  phaseId: string;
  projectId: string;
  status: string;
  waveCount: number;
  currentWave: number | null;
  plans: PlanEntry[];
}
```

---

## Implementation Order

The module was built in 8 phases, each delivering a testable capability that unblocked the next:

1. **Foundation** — SQLite schema (v20), `PlanningStore` CRUD with transactions, pure FSM with all 11 states and 15 transitions, `PlanningConfig` Zod schema in `taxis/schema.ts`.

2. **Orchestrator & Entry** — `DianoiaOrchestrator` core (handle, abandon, confirmResume), ~~`/plan` slash command~~ (**not built** → #323), ~~`aletheia plan` CLI subcommand~~ (**not built** → #324), `detectPlanningIntent()` for natural-language planning detection (**partial** — exists in `intent.ts`, wired in `context.ts:289`, but only injects soft prompt suggestion, not full orchestrator activation).

3. **Project Context & API** — Conversational questioning loop (`processAnswer`, `getNextQuestion`, `confirmSynthesis`), v21 migration for `project_context`, Pylon API routes for listing and inspecting projects. ~~Legacy tool deprecation~~ (**not done** — `plan_create` still active and is currently the only entry point for planning).

4. **Research Pipeline** — `ResearchOrchestrator` spawning four parallel dimension agents (stack, features, architecture, pitfalls) via `sessions_dispatch`, v22 migration for research status, synthesis agent, timeout surfacing, FSM advance to requirements.

5. **Requirements Definition** — `RequirementsOrchestrator` with interactive category scoping, REQ-ID assignment, v23 migration for `rationale`, `plan_requirements` tool (5 actions), `completeRequirements()` FSM wiring.

6. **Roadmap & Phase Planning** — `RoadmapOrchestrator` generating phases from requirements (bottom-up), depth-calibrated planning agent, plan checker validation loop, `plan_roadmap` tool (4 actions), `/roadmap` API route.

7. **Execution Orchestration** — `ExecutionOrchestrator` with wave-based parallelism, dependency graph computation, cascade-skip for failed plans, v24 spawn records, zombie detection, `plan_execute` tool (7 actions), execution API routes.

8. **Verification & Checkpoints** — `GoalBackwardVerifier` (goal-backward gap analysis, gap-closure plan generation), `CheckpointSystem` (3-tier risk evaluation, YOLO mode auto-approval, true-blocker category), v25 migration for checkpoint risk fields and verification result, `plan_verify` tool (5 actions).

---

## Success Criteria

1. Spec document `docs/specs/31_dianoia.md` exists with all 7 required sections: Problem, Design, SQLite Schema, State Machine, API Surface, Implementation Order, and Success Criteria.
2. `CONTRIBUTING.md` is updated to document Dianoia module conventions, including migration propagation, `exactOptionalPropertyTypes` usage, `oxlint require-await` compliance, and orchestrator registration pattern.
3. Integration test at `infrastructure/runtime/src/dianoia/dianoia.integration.test.ts` exercises the full pipeline from `idle` to `complete` with mocked `sessions_dispatch`.
4. `npx tsc --noEmit` passes with zero new type errors across the entire `infrastructure/runtime/src/` codebase.
5. `npx oxlint src/` passes with zero new lint errors across the entire `infrastructure/runtime/src/` codebase.
