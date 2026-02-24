---
phase: 05-requirements-definition
plan: "02"
subsystem: dianoia
tags: [requirements, orchestrator, tool, planning, fsm]
dependency_graph:
  requires:
    - 05-01  # PlanningStore.updateRequirement, PLANNING_REQUIREMENT_NOT_FOUND, v23 migration
    - 04-02  # ResearchOrchestrator.transitionToRequirements (synthesized research)
  provides:
    - RequirementsOrchestrator (full requirements scoping loop)
    - plan_requirements tool (5-action dispatch)
    - REQUIREMENTS_COMPLETE FSM transition from requirements → roadmap
  affects:
    - aletheia.ts (new tool registration)
    - dianoia/index.ts (new exports)
    - dianoia/orchestrator.ts (completeRequirements method)
tech_stack:
  added: []
  patterns:
    - REQ-ID auto-increment within category (parse -NN suffix from existing rows, default 0)
    - Promise.resolve() wrapping for sync ToolHandler.execute() without async keyword
    - Re-presentation safety: persistCategory finds MAX existing reqId number and continues from there
key_files:
  created:
    - infrastructure/runtime/src/dianoia/requirements.ts
    - infrastructure/runtime/src/dianoia/requirements-tool.ts
    - infrastructure/runtime/src/dianoia/requirements.test.ts
  modified:
    - infrastructure/runtime/src/dianoia/orchestrator.ts
    - infrastructure/runtime/src/dianoia/index.ts
    - infrastructure/runtime/src/aletheia.ts
decisions:
  - "Promise.resolve() used in ToolHandler.execute() body (no async keyword) to satisfy oxlint require-await while returning Promise<string>"
  - "persistCategory finds MAX existing reqId number by parsing -NN suffix — enables safe re-presentation without duplicate IDs"
  - "formatCategoryPresentation omits differentiators section when differentiators array is empty"
  - "description user-centric enforcement: prefix 'User can ' only when description lacks observable action verbs"
  - "PLANNING_V23_MIGRATION added to index.ts exports alongside other schema exports"
metrics:
  duration: "4 min"
  completed: "2026-02-24"
  tasks_completed: 2
  files_created: 3
  files_modified: 3
---

# Phase 05 Plan 02: RequirementsOrchestrator and plan_requirements Tool Summary

RequirementsOrchestrator + plan_requirements tool: 5-action scoping loop with REQ-ID generation, coverage gate, and REQUIREMENTS_COMPLETE FSM transition.

## What Was Built

### Task 1: RequirementsOrchestrator class and unit tests

**`infrastructure/runtime/src/dianoia/requirements.ts`**

Exports:
- `FeatureProposal` — feature within a category (name, description, isTableStakes, proposedTier, proposedRationale)
- `CategoryProposal` — category with tableStakes + differentiators feature arrays
- `ScopingDecision` — confirmed tier decision per feature (name, tier, rationale)
- `RequirementsOrchestrator` class

Methods:
- `getSynthesis(projectId)` — reads synthesis row from planning_research; returns null when research was skipped
- `formatCategoryPresentation(category)` — builds markdown with table stakes + differentiators sections
- `persistCategory(projectId, category, decisions)` — generates REQ-IDs (`CATEGORY-NN`), re-presentation safe, enforces user-centric descriptions, sets rationale null for v1/v2
- `updateRequirement(projectId, reqId, updates)` — finds by reqId, delegates to store using DB row id; throws PLANNING_REQUIREMENT_NOT_FOUND if not found
- `validateCoverage(projectId, presentedCategories)` — requires 1+ v1 AND all presented categories covered
- `transitionToRoadmap(projectId)` — fires REQUIREMENTS_COMPLETE FSM event

**`infrastructure/runtime/src/dianoia/requirements.test.ts`**

8 tests across 3 describe blocks:
- `getSynthesis`: returns synthesis when present, returns null on skip path
- `persistCategory`: correct REQ-IDs starting at 01, continues from MAX on re-presentation, rationale null for v1/v2 and non-null for out-of-scope
- `validateCoverage`: false when no v1 exists, false when category uncovered, true when all conditions met

### Task 2: plan_requirements tool, orchestrator wiring, registration

**`infrastructure/runtime/src/dianoia/requirements-tool.ts`**

`createPlanRequirementsTool(orchestrator, requirementsOrchestrator)` — 5-action ToolHandler:

| Action | Behavior |
|--------|----------|
| `present_category` | Returns synthesis content or null-path guidance for category derivation |
| `persist_category` | Calls persistCategory, returns coverage gate status |
| `update_requirement` | Calls updateRequirement, returns updated reqId and change summary |
| `check_coverage` | Calls validateCoverage, returns covered boolean + message |
| `complete` | Validates coverage gate first; calls completeRequirements on success |

**`infrastructure/runtime/src/dianoia/orchestrator.ts`**

Added `completeRequirements(projectId, nousId, sessionId): string`:
- Calls `store.updateProjectState(projectId, transition("requirements", "REQUIREMENTS_COMPLETE"))`
- Emits `"planning:phase-complete"` with `{ projectId, nousId, sessionId, phase: "requirements" }`
- Returns `"Requirements confirmed. Advancing to roadmap generation."`

**`infrastructure/runtime/src/dianoia/index.ts`**

Added exports: `RequirementsOrchestrator`, `CategoryProposal`, `FeatureProposal`, `ScopingDecision`, `createPlanRequirementsTool`, `PLANNING_V23_MIGRATION`

**`infrastructure/runtime/src/aletheia.ts`**

Instantiates `RequirementsOrchestrator(store.getDb())` and registers `createPlanRequirementsTool(planningOrchestrator, requirementsOrchestrator)` in the tools array immediately after `planResearchTool`.

## Verification Results

- `npx vitest run src/dianoia/` — 138 tests pass across 6 test files (zero regressions)
- `npx tsc --noEmit` — zero type errors
- `npx oxlint src/dianoia/` — zero errors (3 pre-existing sort-imports warnings in existing files, out of scope)

## Commits

- `7d06050` — `feat(05-02): implement RequirementsOrchestrator with 8 unit tests`
- `27444e5` — `feat(05-02): wire plan_requirements tool, completeRequirements, exports, registration`

## Deviations from Plan

None — plan executed exactly as written.

## Self-Check: PASSED

Files exist:
- FOUND: infrastructure/runtime/src/dianoia/requirements.ts
- FOUND: infrastructure/runtime/src/dianoia/requirements-tool.ts
- FOUND: infrastructure/runtime/src/dianoia/requirements.test.ts

Commits exist:
- FOUND: 7d06050
- FOUND: 27444e5
