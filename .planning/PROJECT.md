# Dianoia — Native Planning System for Aletheia

## What This Is

Dianoia (διάνοια — "thinking-through") is a first-class planning module for Aletheia that transforms complex, multi-session work from ad-hoc conversation into structured, agent-orchestrated execution. It provides the full planning stack — project context, research, requirements, roadmap, phase planning, multi-agent execution, and verification — as native runtime primitives. Triggered by `/plan` command or automatic agent-detected planning intent, both paths route to the same coherent planning runtime. Target deliverable: a production-ready PR to the upstream aletheia repository.

## Core Value

Complex AI work stays coherent from first prompt to merged PR — project state, requirements, and execution history persist across sessions and agents, with multi-agent quality gates at every phase.

## Requirements

### Validated

<!-- Existing capabilities already in Aletheia that Dianoia builds on or supersedes -->

- ✓ `plan_propose` — multi-step plan with human approval gate and cost estimates — existing
- ✓ `plan_create` / `plan_status` / `plan_step_complete` / `plan_step_fail` — structured step tracking with dependency graph — existing
- ✓ `sessions_spawn` — spawn specialist subagents (coder, reviewer, researcher, explorer, runner) — existing
- ✓ `sessions_ask` / `sessions_dispatch` — synchronous and parallel agent delegation — existing
- ✓ Hook system — lifecycle events (`turn:before`, `turn:after`, `tool:called`, etc.) — existing
- ✓ Commands — slash command definitions in `shared/commands/` — existing
- ✓ Blackboard — cross-agent key-value state with TTL — existing
- ✓ Working state — session-scoped task context auto-extracted post-turn — existing
- ✓ Roles — coder, researcher, reviewer, explorer, runner prompt specializations — existing
- ✓ Ephemeral agents — temporary specialists with custom soul — existing

### Active

<!-- What Dianoia adds -->

- [ ] Project-level persistent state across sessions (PROJECT.md, REQUIREMENTS.md, ROADMAP.md, STATE.md equivalents in SQLite)
- [ ] Phase structure — roadmap decomposed into phases, each with goals, requirements mapping, success criteria
- [ ] Multi-agent research pipeline — parallel researcher agents (stack, features, architecture, pitfalls) + synthesizer
- [ ] Requirements scoping — interactive per-category scoping with v1/v2/out-of-scope tracking
- [ ] Roadmap generation — requirement-to-phase mapping with 100% coverage validation
- [ ] Planning quality gates — plan-checker (does this plan achieve the goal?) and verifier (did execution satisfy requirements?)
- [ ] Planning intent detection — turn pipeline detects planning intent and offers structured planning mode
- [ ] `/plan` command entry point — triggers full Dianoia flow from conversation
- [ ] Phase execution orchestration — wave-based parallel plan execution using sessions_dispatch
- [ ] Checkpoint system — human-in-loop approval gates with approve/adjust/skip options
- [ ] Planning state API routes — `/api/planning/*` endpoints in pylon
- [ ] Planning session state machine — idle → questioning → researching → requirements → roadmap → phase-planning → executing → verifying → complete
- [ ] Supersede existing plan tools — plan_propose, plan_create, plan_status, plan_step_complete, plan_step_fail absorbed into Dianoia's execution layer
- [ ] Config persistence — planning preferences (depth, parallelization, quality gates, model profile) persisted per-project
- [ ] Spec document — `docs/specs/31_dianoia.md` following established spec format
- [ ] Test coverage — unit tests for state machine, store operations, intent detection

### Out of Scope

- Full deterministic workflow engine (spec 22/CR-1) — Dianoia is purpose-built; may inform that spec later
- Memory/distillation changes — existing mem0/distillation pipeline untouched
- A2A protocol integration — external agent interop is separate concern
- IDE integration (ACP) — separate spec
- TUI/UI deep integration — basic planning state visibility acceptable; rich UI deferred
- New channel integrations (Signal, webchat) — routing untouched

## Context

**Existing plan system:** `organon/built-in/plan-propose.ts` + `organon/built-in/plan.ts` handle session-scoped, single-level planning. plan_propose pauses turns for human approval; plan_create tracks steps with dependency graph. These are the right low-level primitives but lack the project/phase/research/verification layers that make complex work tractable.

**GSD reference:** The Get-Shit-Done framework (`~/.claude/get-shit-done/`) implements the full planning pipeline as Claude Code skills. It works. Dianoia takes GSD's best ideas and makes them native Aletheia primitives — persisted in SQLite (not markdown files), driven by sessions_spawn instead of the Task tool, integrated with Aletheia's existing hook/event/role infrastructure.

**Similar art reviewed:** CrewAI Flows (event-driven workflows, state persistence), LangGraph (state graph with checkpoints, reducer pattern for parallel outputs), gap analysis (spec 17) recommends CrewAI Flows pattern adapted to Aletheia's event bus — Dianoia is the purpose-built planning instantiation of that recommendation.

**Oikia alignment:** Oikia is forkwright's local Aletheia deployment for Acme work. This PR targets upstream aletheia. Oikia gains Dianoia automatically when it tracks upstream. Oikia-specific planning configuration lives in `ergon/shared/` (local only, not in PR).

**Module naming:** Following the system in `naming_system.md` — Dianoia (διάνοια) = "thinking-through". L1: planning process that works step by step. L4: the mode of mind that must traverse — cannot skip to conclusion. Resonance: "analysis pipelines; recursive processes; anything that honors the journey over the arrival."

## Constraints

- **Architecture**: Native TypeScript runtime module, not commands-only layer — planning state lives in SQLite via mneme store extensions
- **Module pattern**: Greek name `dianoia`, follows existing module conventions (createLogger, AletheiaError hierarchy, trySafeAsync)
- **Imports**: `.js` extensions, ESM, no circular deps to existing modules
- **Testing**: vitest, behavior-not-implementation, no empty catch blocks
- **PR quality**: Must pass existing test suite, `npx tsc --noEmit` clean, `npx oxlint src/` clean
- **Backward compat**: plan_create/plan_propose tools deprecated but not removed in this PR — migration path documented
- **Anthropic API**: Dianoia spawns subagents via sessions_spawn (existing primitive), not direct Anthropic API calls

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Module name: Dianoia | Fits naming system — "thinking-through", mode of mind that must traverse | — Pending |
| State in SQLite via mneme | Consistent with all other Aletheia persistence; survives restarts | — Pending |
| Entry: command + intent detection | User explicitly invokes or agent detects — both route same flow | — Pending |
| Replace plan_propose/create | Nothing sacred; Dianoia is the world-class replacement | — Pending |
| Build on sessions_spawn not new primitives | Existing multi-agent primitives are correct; add orchestration layer above | — Pending |
| Spec 22 workflow engine deferred | Dianoia is purpose-built planning, not general workflow engine; informs spec 22 later | — Pending |

---
*Last updated: 2026-02-23 after initialization*
