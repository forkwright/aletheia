# Requirements: Dianoia

**Defined:** 2026-02-23
**Core Value:** Complex AI work stays coherent from first prompt to merged PR -- project state, requirements, and execution history persist across sessions and agents, with multi-agent quality gates at every phase.

---

## v1 Requirements

### Foundation -- State & Persistence

- [x] **FOUND-01**: User's planning project state persists across sessions in SQLite (project survives runtime restart)
- [x] **FOUND-02**: Planning project has a well-defined state machine with named states (idle, questioning, researching, requirements, roadmap, phase-planning, executing, verifying, complete, blocked, abandoned)
- [x] **FOUND-03**: All multi-step planning state mutations are wrapped in explicit SQLite transactions (no partial-write corruption)
- [x] **FOUND-04**: Planning state stores a context hash at project creation and checks freshness before phase execution
- [x] **FOUND-05**: Planning schema added via SQLite migration v20 following existing mneme migration pattern
- [x] **FOUND-06**: Planning store (`dianoia/store.ts`) is a separate class from SessionStore but shares the same `db` instance

### Entry & Discovery

- [ ] **ENTRY-01**: User can initiate planning via `/plan` slash command in any session
- [ ] **ENTRY-02**: Agent detects planning intent in the turn pipeline via `turn:before` hook and offers to engage planning mode
- [ ] **ENTRY-03**: Both entry paths (command and agent-detected) route to the same DianoiaOrchestrator state machine
- [ ] **ENTRY-04**: Planning session is associated with the initiating nous and session, resumable from any later session with the same nous
- [ ] **ENTRY-05**: `aletheia plan` CLI subcommand starts planning mode directly

### Project Context Gathering

- [ ] **PROJ-01**: User can answer questions about their project through natural conversation (agent asks, user answers inline)
- [ ] **PROJ-02**: Project context is persisted to SQLite after questioning phase completes
- [ ] **PROJ-03**: Agent synthesizes project description, core value, constraints, and key decisions from the conversation
- [ ] **PROJ-04**: Project context is injected into agent working-state and survives distillation

### Research Pipeline

- [ ] **RESR-01**: Agent can spawn 4 parallel domain researchers (stack, features, architecture, pitfalls dimensions) via sessions_dispatch
- [ ] **RESR-02**: Research results are stored in `planning_research` table per-dimension
- [ ] **RESR-03**: A synthesizer agent produces a consolidated research summary after all 4 researchers complete
- [ ] **RESR-04**: Research can be skipped (user already knows the domain)
- [ ] **RESR-05**: Researcher subagents use ephemeralSoul definitions specific to each research dimension
- [ ] **RESR-06**: Research phase has a timeout; partial results are captured if one researcher stalls

### Requirements Definition

- [ ] **REQS-01**: Agent presents features by category with table-stakes vs. differentiator classification
- [ ] **REQS-02**: User can scope each category (v1 / v2 / out of scope) through structured interaction
- [ ] **REQS-03**: Requirements are assigned REQ-IDs in format `[CATEGORY]-[NUMBER]` (e.g., AUTH-01, CONT-02)
- [ ] **REQS-04**: Requirements are persisted to `planning_requirements` table with tier (v1/v2/out-of-scope)
- [ ] **REQS-05**: Requirements are user-centric, specific, and testable ("User can X", not "System does Y")
- [ ] **REQS-06**: Out-of-scope requirements include rationale (prevents re-adding later)
- [ ] **REQS-07**: Requirements coverage is validated before advancing (all v1 requirements must map to phases)

### Roadmap Generation

- [ ] **ROAD-01**: Roadmap generation spawns a specialist agent that produces phases from requirements (bottom-up, not top-down)
- [ ] **ROAD-02**: Every v1 requirement maps to exactly one phase; roadmap agent validates 100% coverage
- [ ] **ROAD-03**: Each phase has: name, goal (one sentence), mapped REQ-IDs, and 2-5 observable success criteria
- [ ] **ROAD-04**: Phases are stored in `planning_phases` table with order, goal, and success criteria
- [ ] **ROAD-05**: User can adjust the roadmap before it's committed (interactive mode) or it's auto-committed (YOLO mode)
- [ ] **ROAD-06**: Roadmap is accessible via `/api/planning/projects/:id/roadmap`

### Phase Planning

- [ ] **PHAS-01**: Planning state machine advances through phases sequentially; current phase is tracked in `planning_projects`
- [ ] **PHAS-02**: For each phase, a planning agent produces a PLAN with steps, dependencies, and acceptance criteria
- [ ] **PHAS-03**: Plan checker agent verifies the plan will achieve the phase goal (goal-backward analysis) before execution starts
- [ ] **PHAS-04**: Phase plans are stored in `planning_phases.plan` JSON column
- [ ] **PHAS-05**: User can skip the plan checker for a given phase (override in config)
- [ ] **PHAS-06**: Phase planning respects project depth config (quick: 1-3 plans per phase, comprehensive: 5-10)

### Execution Orchestration

- [ ] **EXEC-01**: Phase execution uses wave-based parallelism -- independent plans within a phase run concurrently via sessions_dispatch
- [ ] **EXEC-02**: Wave executor computes dependency graph before execution; dependent plans wait for prerequisites
- [ ] **EXEC-03**: Subagent spawn records are stored in SQLite (not just in-memory) to survive restarts and enable zombie detection
- [ ] **EXEC-04**: Failed plans cascade-skip dependent plans; non-dependent plans continue executing
- [ ] **EXEC-05**: Execution can be paused at phase boundaries and resumed in a later session
- [ ] **EXEC-06**: Execution progress is accessible via `/api/planning/projects/:id/phases/:phaseId/status`

### Verification

- [ ] **VERI-01**: After each phase executes, a verifier agent performs goal-backward verification (truths -> artifacts -> wiring)
- [ ] **VERI-02**: Verifier reports: phase goal met / partially met / not met, with specific gap analysis
- [ ] **VERI-03**: If phase goal not met, agent proposes gap-closure steps before advancing
- [ ] **VERI-04**: User can override verification failure and advance anyway (recorded in `planning_checkpoints`)
- [ ] **VERI-05**: Verification can be disabled per-project in planning config

### Human-in-Loop Checkpoints

- [ ] **CHKP-01**: Checkpoints fire on the event bus as `planning:checkpoint` events
- [ ] **CHKP-02**: Checkpoints are risk-based: low-cost reversible decisions auto-approved; high-cost or irreversible decisions require human input
- [ ] **CHKP-03**: Checkpoint decisions are persisted to `planning_checkpoints` table (full audit trail)
- [ ] **CHKP-04**: In YOLO mode, all checkpoints except blocking errors are auto-approved
- [ ] **CHKP-05**: User can inject notes/constraints into a checkpoint decision (context captured with the decision)

### API & Integration

- [ ] **INTG-01**: Dianoia exposes `/api/planning/projects` CRUD routes in pylon
- [ ] **INTG-02**: Planning project state is accessible via GET `/api/planning/projects/:id`
- [ ] **INTG-03**: Planning fires events on Aletheia's event bus (`planning:project-created`, `planning:phase-started`, `planning:phase-complete`, `planning:checkpoint`, `planning:complete`)
- [ ] **INTG-04**: Planning state is injected into agent working-state so the nous knows the active planning context
- [ ] **INTG-05**: Existing `plan_create`/`plan_propose` tools are marked deprecated but not removed (documented migration path to Dianoia tools)

### Planning Config

- [ ] **CONF-01**: Planning config stored per-project in `planning_projects.config` JSON column (depth, parallelization, model profile, workflow flags)
- [ ] **CONF-02**: Default planning config read from `~/.aletheia/aletheia.json` `planning` section
- [ ] **CONF-03**: Config supports: depth (quick/standard/comprehensive), parallelization (true/false), research (true/false), plan_check (true/false), verifier (true/false), mode (yolo/interactive)

### Spec & Documentation

- [ ] **DOCS-01**: Spec document `docs/specs/31_dianoia.md` follows established Aletheia spec format (Problem, Design, Implementation Order, Success Criteria)
- [ ] **DOCS-02**: Spec includes SQLite schema definitions, state machine diagram, and API surface
- [ ] **DOCS-03**: CONTRIBUTING.md updated to note Dianoia module conventions

### Tests

- [x] **TEST-01**: Unit tests for state machine transitions (pure FSM -- all paths covered)
- [x] **TEST-02**: Unit tests for PlanningStore CRUD operations using in-memory SQLite
- [ ] **TEST-03**: Unit tests for intent detection hook (true positive and false positive scenarios)
- [ ] **TEST-04**: Integration test for the full planning pipeline (mock sessions_dispatch)
- [ ] **TEST-05**: `npx tsc --noEmit` passes with zero new type errors
- [ ] **TEST-06**: `npx oxlint src/` passes with zero new lint errors

---

## v2 Requirements

### Advanced Execution

- **EXEC-V2-01**: Time-travel debugging -- browse planning checkpoint history and restore state to any previous checkpoint
- **EXEC-V2-02**: Background execution -- planning phases execute as daemon tasks while user does other work
- **EXEC-V2-03**: Cross-project dependency tracking -- phase in one project can depend on phase in another

### TUI/UI Integration

- **UI-V2-01**: TUI dashboard panel shows active planning project state (current phase, progress bar)
- **UI-V2-02**: Web UI planning view -- visual roadmap with phase status, drag-to-reorder
- **UI-V2-03**: Real-time checkpoint notification in TUI/UI (not just in the chat stream)

### Advanced Verification

- **VERI-V2-01**: Verifier computes a confidence score per success criterion (0.0-1.0), not just pass/fail
- **VERI-V2-02**: Verifier suggests specific follow-up tasks for partially-met criteria
- **VERI-V2-03**: Cross-phase requirement traceability -- show which requirements were validated by which phase

### Project Evolution

- **EVOL-V2-01**: After all phases complete, agent proposes next milestone based on unvalidated requirements
- **EVOL-V2-02**: Requirements can be promoted v2 to v1 during project execution with automatic roadmap extension
- **EVOL-V2-03**: Project evolution log tracks requirement lifecycle (active -> validated -> out-of-scope)

---

## Out of Scope

| Feature | Reason |
|---------|--------|
| General deterministic workflow engine (spec 22/CR-1) | Dianoia is purpose-built planning; workflow engine is a separate, larger concern |
| Memory/distillation system changes | Existing mem0/distillation pipeline untouched |
| A2A protocol integration | External agent interop is a separate spec |
| ACP / IDE integration | Separate spec (spec 25) |
| New channel integrations (Signal, webchat routing) | Routing untouched |
| Agent fine-tuning from planning traces | RSI L3-4 territory; out of scope for this PR |
| Removing plan_create/plan_propose in this PR | Migration path documented; removal in a subsequent PR |

---

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| FOUND-01 | Phase 1: Foundation | Complete |
| FOUND-02 | Phase 1: Foundation | Complete |
| FOUND-03 | Phase 1: Foundation | Complete |
| FOUND-04 | Phase 1: Foundation | Complete |
| FOUND-05 | Phase 1: Foundation | Complete |
| FOUND-06 | Phase 1: Foundation | Complete |
| ENTRY-01 | Phase 2: Orchestrator & Entry | Pending |
| ENTRY-02 | Phase 2: Orchestrator & Entry | Pending |
| ENTRY-03 | Phase 2: Orchestrator & Entry | Pending |
| ENTRY-04 | Phase 2: Orchestrator & Entry | Pending |
| ENTRY-05 | Phase 2: Orchestrator & Entry | Pending |
| PROJ-01 | Phase 3: Project Context & API | Pending |
| PROJ-02 | Phase 3: Project Context & API | Pending |
| PROJ-03 | Phase 3: Project Context & API | Pending |
| PROJ-04 | Phase 3: Project Context & API | Pending |
| RESR-01 | Phase 4: Research Pipeline | Pending |
| RESR-02 | Phase 4: Research Pipeline | Pending |
| RESR-03 | Phase 4: Research Pipeline | Pending |
| RESR-04 | Phase 4: Research Pipeline | Pending |
| RESR-05 | Phase 4: Research Pipeline | Pending |
| RESR-06 | Phase 4: Research Pipeline | Pending |
| REQS-01 | Phase 5: Requirements Definition | Pending |
| REQS-02 | Phase 5: Requirements Definition | Pending |
| REQS-03 | Phase 5: Requirements Definition | Pending |
| REQS-04 | Phase 5: Requirements Definition | Pending |
| REQS-05 | Phase 5: Requirements Definition | Pending |
| REQS-06 | Phase 5: Requirements Definition | Pending |
| REQS-07 | Phase 5: Requirements Definition | Pending |
| ROAD-01 | Phase 6: Roadmap & Phase Planning | Pending |
| ROAD-02 | Phase 6: Roadmap & Phase Planning | Pending |
| ROAD-03 | Phase 6: Roadmap & Phase Planning | Pending |
| ROAD-04 | Phase 6: Roadmap & Phase Planning | Pending |
| ROAD-05 | Phase 6: Roadmap & Phase Planning | Pending |
| ROAD-06 | Phase 6: Roadmap & Phase Planning | Pending |
| PHAS-01 | Phase 6: Roadmap & Phase Planning | Pending |
| PHAS-02 | Phase 6: Roadmap & Phase Planning | Pending |
| PHAS-03 | Phase 6: Roadmap & Phase Planning | Pending |
| PHAS-04 | Phase 6: Roadmap & Phase Planning | Pending |
| PHAS-05 | Phase 6: Roadmap & Phase Planning | Pending |
| PHAS-06 | Phase 6: Roadmap & Phase Planning | Pending |
| EXEC-01 | Phase 7: Execution Orchestration | Pending |
| EXEC-02 | Phase 7: Execution Orchestration | Pending |
| EXEC-03 | Phase 7: Execution Orchestration | Pending |
| EXEC-04 | Phase 7: Execution Orchestration | Pending |
| EXEC-05 | Phase 7: Execution Orchestration | Pending |
| EXEC-06 | Phase 7: Execution Orchestration | Pending |
| VERI-01 | Phase 8: Verification & Checkpoints | Pending |
| VERI-02 | Phase 8: Verification & Checkpoints | Pending |
| VERI-03 | Phase 8: Verification & Checkpoints | Pending |
| VERI-04 | Phase 8: Verification & Checkpoints | Pending |
| VERI-05 | Phase 8: Verification & Checkpoints | Pending |
| CHKP-01 | Phase 8: Verification & Checkpoints | Pending |
| CHKP-02 | Phase 8: Verification & Checkpoints | Pending |
| CHKP-03 | Phase 8: Verification & Checkpoints | Pending |
| CHKP-04 | Phase 8: Verification & Checkpoints | Pending |
| CHKP-05 | Phase 8: Verification & Checkpoints | Pending |
| INTG-01 | Phase 3: Project Context & API | Pending |
| INTG-02 | Phase 3: Project Context & API | Pending |
| INTG-03 | Phase 3: Project Context & API | Pending |
| INTG-04 | Phase 3: Project Context & API | Pending |
| INTG-05 | Phase 3: Project Context & API | Pending |
| CONF-01 | Phase 1: Foundation | Pending |
| CONF-02 | Phase 1: Foundation | Pending |
| CONF-03 | Phase 1: Foundation | Pending |
| DOCS-01 | Phase 9: Polish & Migration | Pending |
| DOCS-02 | Phase 9: Polish & Migration | Pending |
| DOCS-03 | Phase 9: Polish & Migration | Pending |
| TEST-01 | Phase 1: Foundation | Complete |
| TEST-02 | Phase 1: Foundation | Complete |
| TEST-03 | Phase 2: Orchestrator & Entry | Pending |
| TEST-04 | Phase 9: Polish & Migration | Pending |
| TEST-05 | Phase 9: Polish & Migration | Pending |
| TEST-06 | Phase 9: Polish & Migration | Pending |

**Coverage:**
- v1 requirements: 60 total
- Mapped to phases: 60
- Unmapped: 0

---
*Requirements defined: 2026-02-23*
*Last updated: 2026-02-23 after roadmap creation*
