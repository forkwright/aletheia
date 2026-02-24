# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-23)

**Core value:** Complex AI work stays coherent from first prompt to merged PR -- project state, requirements, and execution history persist across sessions and agents, with multi-agent quality gates at every phase.
**Current focus:** Phase 9: Polish and Migration

## Current Position

Phase: 9 of 9 (Polish and Migration) — COMPLETE
Plan: 4 of 4 in current phase — all 4 plans complete
Status: Phase 9 complete — PlanningStatusLine + PlanningPanel + ChatView wiring (09-04); TEST-05 satisfied; all 9 phases done
Last activity: 2026-02-24 -- Phase 9 Plan 4 complete (Svelte 5 status pill UI, human-verified in running UI)

Progress: [██████████] 100%

## Performance Metrics

**Velocity:**
- Total plans completed: 22
- Average duration: 4 min
- Total execution time: 1.02 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-foundation | 3/3 | 14 min | 5 min |
| 02-orchestrator-and-entry | 3/3 | 7 min | 2 min |
| 03-project-context-and-api | 4/4 | 9 min | 2 min |
| 04-research-pipeline | 2/2 | 6 min | 3 min |
| 05-requirements-definition | 2/? | 6 min | 3 min |
| 06-roadmap-phase-planning | 3/3 | 16 min | 5 min |
| 07-execution-orchestration | 4/4 | 28 min | 7 min |
| 08-verification-checkpoints | 4/4 | 13 min | 3.25 min |

**Recent Trend:**
- Last 5 plans: 3 min, 1 min, 3 min, 10 min, 8 min
- Trend: stable

*Updated after each plan completion*
| Phase 08-verification-checkpoints P03 | 2 | 2 tasks | 2 files |
| Phase 08-verification-checkpoints P04 | 3 | 2 tasks | 4 files |
| Phase 09-polish-migration P03 | 1 | 2 tasks | 4 files |
| Phase 09-polish-migration P02 | 2 | 1 task | 1 file |
| Phase 09-polish-migration P04 | 15 | 3 tasks | 3 files |

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [Roadmap]: 9 phases derived from 14 requirement categories; research suggests 7 but comprehensive depth splits Requirements from Roadmap and groups Verification with Checkpoints
- [Roadmap]: Tests distributed to the phases that create the code they test (TEST-01/02 in Phase 1, TEST-03 in Phase 2, TEST-04/05/06 in Phase 9)
- [Roadmap]: Phase 4 (Research) depends only on Phase 2, enabling parallel execution with Phase 3
- [01-01]: planning_checkpoints has no updated_at (append-only, decisions immutable once recorded)
- [01-01]: contextHash is SHA-256(goal|nousId|createdAt) truncated to 16 hex -- not recomputable without original createdAt
- [01-01]: PlanningStore uses injected-db pattern; wiring to SessionStore db deferred to Phase 2
- [01-02]: NEXT_PHASE + ALL_PHASES_COMPLETE split (not single PHASE_PASSED) -- orchestrator controls "are there more phases?", FSM stays self-contained
- [01-02]: VALID_TRANSITIONS and TRANSITION_RESULT kept as two separate structures -- VALID_TRANSITIONS is public API for display, TRANSITION_RESULT is internal lookup
- [01-02]: DianoiaState re-exported from machine.ts via type-only re-export -- consumers import both state and event types from one location
- [Phase 01-03]: Zod schema in taxis/schema.ts is authoritative for PlanningConfig; dianoia/types.ts re-exports PlanningConfigSchema via import type
- [Phase 01-03]: Import direction preserved: dianoia imports from taxis (no circular dependency); PlanningConfigSchema re-export keeps dianoia/index.ts public API intact
- [02-01]: handle() and abandon() are sync (no async) -- oxlint require-await forbids async with no await; confirmResume() also sync; future plans can change signature
- [02-01]: pendingConfirmation flag stored in project.config JSON via cast-through-unknown -- avoids schema migration for ephemeral confirmation state
- [02-01]: DianoiaOrchestrator instantiated in createRuntime() not startRuntime() -- available in tests and CLI commands calling createRuntime() directly
- [Phase 02-02]: execute() in /plan command uses Promise.resolve(sync) not async — satisfies CommandHandler interface without oxlint require-await
- [Phase 02-02]: getPlanningOrchestrator() getter added to NousManager — avoids adding orchestrator to AletheiaRuntime interface
- [02-03]: Two-signal detection required for build/create verbs (project-scale noun must co-occur) to prevent false positives
- [02-03]: Explicit phrases (help me plan, new project, /plan) are single-signal sufficient
- [02-03]: Intent offer is else-branch of activeProject check — clean mutual exclusion, single pass
- [03-01]: FSM event questioning->researching is START_RESEARCH (not COMPLETE_QUESTIONING — that event doesn't exist in machine.ts)
- [03-01]: planning:checkpoint event not in EventName union; confirmSynthesis emits planning:phase-started instead
- [03-01]: confirmSynthesis preserves rawTranscript from existing context, not from caller's synthesizedContext
- [03-01]: exactOptionalPropertyTypes requires conditional spread for optional array fields in merged context objects
- [03-02]: exactOptionalPropertyTypes requires conditional spread for optional RouteDeps fields (planningOrchestrator) — direct undefined assignment fails type check
- [03-02]: listAllProjects() and getProject() added as thin public accessors on DianoiaOrchestrator delegating to store — routes never reach through to store directly
- [03-02]: GET /api/planning/projects returns summary fields only; full snapshot only on /:id
- [03-03]: getNextQuestion(projectId) called with activeProject.id (not nousId) — matches orchestrator signature
- [03-03]: nextQuestion guard uses state === 'questioning' before calling getNextQuestion — avoids unnecessary DB reads
- [03-03]: Planning Question rendered as ## Planning Question H2 section for clear LLM salience
- [03-03]: No distillation pipeline changes needed — context block re-reads from DB each turn (PROJ-04 satisfied structurally)
- [03-04]: deprecationWarning placed as JSON key inside JSON.stringify payload — never prepended as text — preserves PLAN_PROPOSED_MARKER JSON.parse compatibility
- [03-04]: plan_status, plan_step_complete, plan_step_fail left unchanged — deprecation deferred to Phase 9 per CONTEXT.md
- [04-01]: ResearchOrchestrator takes (db, dispatchTool) and creates own PlanningStore internally — matches DianoiaOrchestrator pattern
- [04-01]: context field used for soul injection in sessions_dispatch (not ephemeralSoul — that param does not exist on DispatchTask interface)
- [04-01]: planning_research.status DEFAULT 'complete' — backward compatible with existing rows, no data migration needed
- [04-01]: plan_research skip branch returns {status: skipped} — skipResearch() deferred to plan 04-02
- [04-01]: test makeDb() must include all migrations through current version — V22 added to store.test.ts and researcher.test.ts
- [04-02]: ResearchOrchestrator.transitionToRequirements() co-located with research completion -- research-tool drives sequence, orchestrator owns state
- [04-02]: synthesizeResearch() reuses dispatchTool for synthesis dispatch -- no separate spawn tool needed
- [04-02]: koina/safe.ts trySafeAsync is (label, fn, fallback) not Result pattern -- use direct try/catch for error boundaries in tool execute() bodies
- [05-01]: PLANNING_REQUIREMENT_NOT_FOUND added to error-codes.ts alongside other PLANNING_* codes — not inlined as string literal
- [05-01]: updateRequirement uses dynamic SET construction (sets[]/vals[] arrays) — allows updating tier-only, rationale-only, or both atomically in one UPDATE statement
- [05-01]: createRequirement INSERT always lists rationale column explicitly (passing null when not provided) — avoids conditional column-list logic
- [05-02]: Promise.resolve() wrapping used in ToolHandler.execute() (no async keyword) to satisfy oxlint require-await while returning Promise<string>
- [05-02]: persistCategory finds MAX existing reqId number by parsing -NN suffix — enables safe re-presentation without duplicate IDs
- [05-02]: description user-centric enforcement prefixes 'User can' only when description lacks observable action verbs — minimal intervention
- [06-01]: depthToInstruction is a public method on RoadmapOrchestrator — directly testable without indirect dispatch
- [06-01]: commitRoadmap stores this.db as instance field alongside this.store — required for db.transaction() wrapper spanning multiple createPhase() calls
- [06-01]: checkPlan dispatch failure returns {pass: true} — best-effort avoids checker blocking plan generation
- [06-01]: planPhase reads depth from store.getProjectOrThrow(projectId).config.depth — orchestrator owns depth selection
- [06-02]: plan_roadmap generate commits roadmap draft in both interactive and yolo modes (write-on-generate); yolo additionally calls completeRoadmap immediately
- [06-02]: plan_phases uses sequential reduce chain (not Promise.all) — PHAS-01 requires sequential phase planning
- [06-02]: completeRoadmap and advanceToExecution on DianoiaOrchestrator (not RoadmapOrchestrator) — FSM transitions are orchestrator's domain
- [06-03]: No additional JSON.parse in routes.ts — PlanningStore.mapPhase() already parses requirements/successCriteria from SQLite JSON strings into string[]
- [06-03]: RoadmapOrchestrator constructed with (store.getDb(), dispatchTool) — matches ResearchOrchestrator pattern; dispatchTool already available at wiring point
- [07-01]: ExecutionOrchestrator takes (db, dispatchTool) with db not stored as private field — passed to PlanningStore constructor only, avoids TS6138 unused-property error
- [07-01]: PLANNING_SPAWN_NOT_FOUND added to error-codes.ts alongside other PLANNING_* codes — not inlined as string
- [07-01]: computeWaves uses PhasePlan.dependencies (plan-to-plan), not PlanStep.dependsOn (step-to-step) as the unit of parallelism
- [07-01]: Cascade-skip is direct-dependents-only: Plan A fails skips B (B depends A), but C (depends B) continues unless B also fails
- [07-01]: Spawn records created BEFORE dispatch so crash-before-dispatch leaves a recoverable trace
- [07-02]: plan_execute execute() method returns handleAction() directly (async fn) — no async keyword on outer, satisfies oxlint require-await
- [07-02]: phaseId accepted in plan_execute input schema but not used as local — executePhase operates on projectId only (no phaseId param in actual method)
- [07-02]: nousId/sessionId in plan_execute fall back to context fields when not provided in input — callers don't repeat context
- [07-02]: All 7 switch cases in plan_execute wrapped in single try/catch returning JSON error — consistent error surface
- [07-03]: executionOrchestrator stored on NousManager via setter/getter — matches planningOrchestrator pattern; server.ts retrieves via manager.getExecutionOrchestrator()
- [07-03]: RouteDeps.executionOrchestrator uses conditional spread in server.ts — exactOptionalPropertyTypes requires this (consistent with planningOrchestrator)
- [07-03]: Routes return 503 when executionOrchestrator not available — defensive guard matches existing planning route pattern
- [07-04]: isPaused() combines state===blocked and pause_between_phases===true in one OR — both halt before every wave including wave 0
- [07-04]: reapZombies reads allPhases via store.listPhases() internally — no method signature change, keeps call sites clean
- [07-04]: Zombie cascade uses waveNumber+1 for skipped records — consistent with executePhase cascade pattern using waveIndex+1
- [07-04]: store.createPhase() has no plan param; tests use store.updatePhasePlan() to set dependencies after creation
- [07-04]: pause_between_phases test expects 0 dispatch calls — isPaused fires before every wave including wave 0; plan comment was incorrect
- [08-01]: VerificationResult.overridden uses optional property (overridden?: boolean) not boolean | undefined — exactOptionalPropertyTypes compatibility
- [08-01]: resolveCheckpoint opts is optional second arg object — backwards-compatible; existing callers unaffected
- [08-01]: verificationResult JSON parse inside existing mapPhase() try/catch block — consistent PLANNING_STATE_CORRUPT error surface
- [08-01]: planning:checkpoint inserted between planning:phase-complete and planning:complete in EventName union — logical event ordering
- [08-02]: GoalBackwardVerifier constructor does not store db as private field (avoids TS6138) — matches ExecutionOrchestrator pattern
- [08-02]: generateGapPlans returns PhasePlan & {id, name} extended shape — PhasePlan interface has no id/name; structural typing permits extra properties
- [08-02]: verify() fallback on parse error returns partially-met with summary "(verification unavailable)" — consistent with researcher synthesis fallback
- [08-02]: Phase re-fetched inside runVerifierAgent (not in verify()) — avoids TS6133 unused-variable when disabled branch returns early
- [Phase 08-03]: CheckpointSystem takes (store, config) not (db, config) — store already created in createRuntime() before instantiation
- [Phase 08-03]: true-blocker branch checked first (before riskLevel) — trueBlockerCategory presence is the discriminator, not a riskLevel value
- [Phase 08-03]: vi.spyOn(eventBus, 'emit') preferred over getter spy pattern — simpler, consistent with orchestrator.test.ts
- [Phase 08-04]: planningStore created as separate PlanningStore instance in createRuntime() — shared by CheckpointSystem and plan_verify tool rather than creating duplicates
- [Phase 08-04]: plan_verify action=run calls blockOnVerificationFailure() for both not-met and partially-met — both states halt the FSM in blocked state requiring user action
- [Phase 09-03]: checkpoint.test.ts uses PlanningStore as type-only (no new PlanningStore) — import type is correct
- [Phase 09-03]: PLANNING_V24_MIGRATION and PLANNING_V25_MIGRATION now re-exported from dianoia/index.ts public API
- [09-02]: Integration tests use vitest.integration.config.ts (*.integration.test.ts include) — excluded from unit suite by vitest.config.ts; pre-existing config already handles this
- [09-02]: executePhase second param is ToolContext (not phaseId) — plan pseudocode correction to match actual ExecutionOrchestrator signature
- [09-02]: Error dispatch path uses status: 'error' in dispatch results to trigger execution.ts failed-phase branch; ToolContext mock requires workspace field per interface
- [Phase 09-polish-migration]: Pill polls /api/planning/projects every 5s (coarse); panel polls /:id/execution every 2.5s when open (fine-grained)
- [Phase 09-polish-migration]: Pill hidden for complete and abandoned states — no need to show resolved projects in chat chrome

### Pending Todos

None yet.

### Blockers/Concerns

- CONCERNS.md flags "No Transactional Guarantees for Multi-Step Operations" in existing store -- Dianoia must not inherit this; FOUND-03 requires explicit transactions
- 3-ephemeral-agent hard limit may conflict with 4 parallel researchers — RESOLVED in 04-01: sessions_dispatch (not ephemeral) used for RESR-01, no limit conflict
- Context distillation may eat planning state — RESOLVED in 03-03: context block re-reads from DB each turn, so synthesized fields survive distillation automatically (PROJ-04)

## Session Continuity

Last session: 2026-02-24
Stopped at: Completed 09-04-PLAN.md (PlanningStatusLine, PlanningPanel, ChatView wiring; TEST-05 satisfied; all 9 phases complete)
Resume file: None
