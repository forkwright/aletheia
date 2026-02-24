# Roadmap: Dianoia

## Overview

Dianoia transforms Aletheia from a session-scoped AI assistant into a project-aware planning runtime. The build progresses from persistence primitives (SQLite schema, state machine, store) through orchestration (entry points, project gathering, API surface) into the planning pipeline (research, requirements, roadmap generation, phase planning), then execution infrastructure (wave-based parallelism, verification, checkpoints), and finally migration and documentation. Each phase delivers a testable, coherent capability that unblocks the next.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Foundation** - SQLite schema, pure state machine, planning store, and config persistence (completed 2026-02-23)
- [x] **Phase 2: Orchestrator & Entry** - DianoiaOrchestrator, /plan command, intent detection, CLI subcommand (completed 2026-02-24)
- [ ] **Phase 3: Project Context & API** - Conversational project gathering, API routes, integration wiring
- [ ] **Phase 4: Research Pipeline** - Parallel researcher spawning, synthesis, timeout handling
- [ ] **Phase 5: Requirements Definition** - Interactive scoping, REQ-ID assignment, persistence, coverage validation
- [ ] **Phase 6: Roadmap & Phase Planning** - Roadmap generation from requirements, phase plan production, plan checker
- [ ] **Phase 7: Execution Orchestration** - Wave-based parallel execution, dependency graph, restart resilience
- [ ] **Phase 8: Verification & Checkpoints** - Goal-backward verification, risk-based checkpoints, audit trail
- [ ] **Phase 9: Polish & Migration** - Spec document, legacy tool deprecation, CONTRIBUTING.md update, final lint/type pass

## Phase Details

### Phase 1: Foundation
**Goal**: Planning state persists in SQLite and transitions through a well-defined state machine
**Depends on**: Nothing (first phase)
**Requirements**: FOUND-01, FOUND-02, FOUND-03, FOUND-04, FOUND-05, FOUND-06, CONF-01, CONF-02, CONF-03, TEST-01, TEST-02
**Success Criteria** (what must be TRUE):
  1. A planning project can be created, read, updated, and deleted via PlanningStore, and survives runtime restart
  2. The state machine enforces valid transitions (e.g., idle to questioning allowed, idle to executing rejected) with all 11 states reachable
  3. Multi-step store mutations use explicit SQLite transactions (no partial-write corruption on crash)
  4. Planning config (depth, parallelization, research, plan_check, verifier, mode) is persisted per-project and defaults from aletheia.json
  5. Unit tests pass for all FSM transitions and all PlanningStore CRUD operations
**Plans**: 3 plans

Plans:
- [ ] 01-01-PLAN.md — SQLite migration v20, PlanningStore CRUD, error codes, and unit tests
- [ ] 01-02-PLAN.md — Pure discriminated-union FSM with TDD (all 11 states, all transition paths)
- [ ] 01-03-PLAN.md — PlanningConfig Zod schema in taxis/schema.ts

### Phase 2: Orchestrator & Entry
**Goal**: Users can initiate and resume planning through multiple entry points, all routed to a single orchestrator
**Depends on**: Phase 1
**Requirements**: ENTRY-01, ENTRY-02, ENTRY-03, ENTRY-04, ENTRY-05, TEST-03
**Success Criteria** (what must be TRUE):
  1. User can type `/plan` in any session and a planning project is created (or resumed if one exists for this nous)
  2. Agent detects planning intent in natural conversation via turn:before hook and offers structured planning mode
  3. `aletheia plan` CLI subcommand starts planning mode directly
  4. A planning session started in one session is resumable from any later session with the same nous
  5. Intent detection unit tests cover true-positive and false-positive scenarios
**Plans**: 3 plans

Plans:
- [x] 02-01-PLAN.md — DianoiaOrchestrator core, SessionStore.getDb(), planning EventNames, RuntimeServices wiring (completed 2026-02-24)
- [ ] 02-02-PLAN.md — /plan slash command, aletheia plan CLI subcommand, orchestrator unit tests
- [ ] 02-03-PLAN.md — detectPlanningIntent() pure function (TDD), context.ts intent injection

### Phase 3: Project Context & API
**Goal**: Planning projects gather context through conversation and expose state via HTTP API
**Depends on**: Phase 2
**Requirements**: PROJ-01, PROJ-02, PROJ-03, PROJ-04, INTG-01, INTG-02, INTG-03, INTG-04, INTG-05
**Success Criteria** (what must be TRUE):
  1. Agent asks project questions inline and synthesizes goal, core value, constraints, and key decisions from the conversation
  2. Project context is persisted to SQLite and injected into agent working-state (survives distillation)
  3. GET `/api/planning/projects` returns all projects; GET `/api/planning/projects/:id` returns full project state
  4. Planning events fire on the event bus (planning:project-created, planning:phase-started, planning:phase-complete, planning:checkpoint, planning:complete)
  5. Existing plan_create/plan_propose tools are marked deprecated with documented migration path (not removed)
**Plans**: 4 plans

Plans:
- [x] 03-01-PLAN.md — Migration v21, ProjectContext persistence, questioning loop (processAnswer, getNextQuestion, confirmSynthesis) (completed 2026-02-24)
- [ ] 03-02-PLAN.md — Pylon API routes: GET /api/planning/projects and GET /api/planning/projects/:id
- [ ] 03-03-PLAN.md — Planning context block enrichment (coreValue, constraints, keyDecisions, next question injection)
- [ ] 03-04-PLAN.md — Legacy tool deprecation: plan_propose and plan_create marked @deprecated with JSON warning keys

### Phase 4: Research Pipeline
**Goal**: Agent can spawn parallel domain researchers and produce a consolidated research summary
**Depends on**: Phase 2
**Requirements**: RESR-01, RESR-02, RESR-03, RESR-04, RESR-05, RESR-06
**Success Criteria** (what must be TRUE):
  1. Four parallel researcher agents (stack, features, architecture, pitfalls) spawn via sessions_dispatch with dimension-specific ephemeralSoul definitions
  2. Research results are stored per-dimension in planning_research table
  3. A synthesizer agent produces a consolidated summary after all researchers complete
  4. Research phase can be skipped entirely (user already knows the domain)
  5. Research has a timeout; partial results are captured and surfaced if one researcher stalls
**Plans**: TBD

Plans:
- [ ] 04-01: ResearchOrchestrator and dimension-specific souls
- [ ] 04-02: Research storage, synthesis, and skip/timeout handling

### Phase 5: Requirements Definition
**Goal**: Agent and user collaboratively define scoped, testable requirements with full persistence
**Depends on**: Phase 3
**Requirements**: REQS-01, REQS-02, REQS-03, REQS-04, REQS-05, REQS-06, REQS-07
**Success Criteria** (what must be TRUE):
  1. Agent presents features by category with table-stakes vs. differentiator classification
  2. User can scope each category as v1, v2, or out-of-scope through structured interaction
  3. Requirements are assigned REQ-IDs in CATEGORY-NUMBER format and persisted to planning_requirements table
  4. All requirements are user-centric and testable; out-of-scope requirements include rationale
  5. Requirements coverage is validated before the state machine advances to roadmap phase
**Plans**: TBD

Plans:
- [ ] 05-01: Requirements scoping interaction flow
- [ ] 05-02: Requirements persistence and coverage validation

### Phase 6: Roadmap & Phase Planning
**Goal**: Requirements are transformed into a phased roadmap with plans, and a plan checker validates goal alignment
**Depends on**: Phase 5
**Requirements**: ROAD-01, ROAD-02, ROAD-03, ROAD-04, ROAD-05, ROAD-06, PHAS-01, PHAS-02, PHAS-03, PHAS-04, PHAS-05, PHAS-06
**Success Criteria** (what must be TRUE):
  1. A specialist agent produces phases from requirements (bottom-up), and every v1 requirement maps to exactly one phase
  2. Each phase has name, goal, mapped REQ-IDs, and 2-5 observable success criteria stored in planning_phases table
  3. User can adjust the roadmap in interactive mode; roadmap auto-commits in YOLO mode
  4. For each phase, a planning agent produces a plan with steps, dependencies, and acceptance criteria (depth-calibrated)
  5. Plan checker agent verifies the plan will achieve the phase goal before execution starts (skippable per config)
**Plans**: TBD

Plans:
- [ ] 06-01: Roadmap generation agent and coverage validation
- [ ] 06-02: Phase planning agent and plan storage
- [ ] 06-03: Plan checker (goal-backward analysis)

### Phase 7: Execution Orchestration
**Goal**: Phase plans execute via wave-based parallelism with dependency ordering and restart resilience
**Depends on**: Phase 6
**Requirements**: EXEC-01, EXEC-02, EXEC-03, EXEC-04, EXEC-05, EXEC-06
**Success Criteria** (what must be TRUE):
  1. Independent plans within a phase run concurrently via sessions_dispatch; dependent plans wait for prerequisites
  2. Wave executor computes dependency graph before execution and respects it throughout
  3. Subagent spawn records are stored in SQLite (survive restart, enable zombie detection)
  4. Failed plans cascade-skip dependent plans while non-dependent plans continue executing
  5. Execution can be paused at phase boundaries and resumed in a later session; progress is accessible via API
**Plans**: TBD

Plans:
- [ ] 07-01: Wave executor and dependency graph
- [ ] 07-02: SQLite-backed spawn records and restart resilience
- [ ] 07-03: Execution status API and pause/resume

### Phase 8: Verification & Checkpoints
**Goal**: Completed phases are verified against their goals, and human-in-loop checkpoints gate high-risk decisions
**Depends on**: Phase 7
**Requirements**: VERI-01, VERI-02, VERI-03, VERI-04, VERI-05, CHKP-01, CHKP-02, CHKP-03, CHKP-04, CHKP-05
**Success Criteria** (what must be TRUE):
  1. After each phase executes, a verifier agent performs goal-backward verification and reports met/partially-met/not-met with gap analysis
  2. If phase goal is not met, agent proposes gap-closure steps before advancing; user can override and advance anyway
  3. Checkpoints fire on the event bus as planning:checkpoint events with risk-based triggering (low-cost reversible auto-approved, high-cost requires human input)
  4. All checkpoint decisions are persisted to planning_checkpoints table with full audit trail including user notes
  5. In YOLO mode, all checkpoints except blocking errors are auto-approved; verification can be disabled per-project
**Plans**: TBD

Plans:
- [ ] 08-01: GoalBackwardVerifier agent
- [ ] 08-02: Risk-based checkpoint system
- [ ] 08-03: Checkpoint persistence and YOLO mode

### Phase 9: Polish & Migration
**Goal**: Dianoia is documented, linted, type-clean, and integration-tested -- ready for PR
**Depends on**: Phase 8
**Requirements**: DOCS-01, DOCS-02, DOCS-03, TEST-04, TEST-05, TEST-06
**Success Criteria** (what must be TRUE):
  1. Spec document `docs/specs/31_dianoia.md` exists with Problem, Design, SQLite schema, state machine diagram, API surface, Implementation Order, and Success Criteria sections
  2. CONTRIBUTING.md is updated to note Dianoia module conventions
  3. Integration test for the full planning pipeline passes (mock sessions_dispatch, exercise idle through complete)
  4. `npx tsc --noEmit` passes with zero new type errors across the entire codebase
  5. `npx oxlint src/` passes with zero new lint errors across the entire codebase
**Plans**: TBD

Plans:
- [ ] 09-01: Spec document (31_dianoia.md)
- [ ] 09-02: Integration test (full pipeline)
- [ ] 09-03: Final type-check, lint, and CONTRIBUTING.md update

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8 -> 9

Note: Phase 4 (Research Pipeline) depends only on Phase 2 and can execute in parallel with Phase 3.

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Foundation | 3/3 | Complete    | 2026-02-23 |
| 2. Orchestrator & Entry | 3/3 | Complete    | 2026-02-24 |
| 3. Project Context & API | 1/4 | In progress | - |
| 4. Research Pipeline | 0/2 | Not started | - |
| 5. Requirements Definition | 0/2 | Not started | - |
| 6. Roadmap & Phase Planning | 0/3 | Not started | - |
| 7. Execution Orchestration | 0/3 | Not started | - |
| 8. Verification & Checkpoints | 0/3 | Not started | - |
| 9. Polish & Migration | 0/3 | Not started | - |
