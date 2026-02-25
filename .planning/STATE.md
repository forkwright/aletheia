# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-25)

**Core value:** A personal AI runtime you fully control — persistent across sessions, extensible through modules, and powerful enough to handle complex multi-agent work without losing context.
**Current focus:** v1.1 Standards & Hardening — Phase 11 in progress

## Current Position

Phase: Phase 11 — tooling-configuration-pre-commit-coverage
Plan: 04 of 05 complete
Status: Plan 11-04 complete — ruff 0.15.2 + pyright 1.1.408 installed as uv dev dependencies; pyproject.toml configured; baselines: 152 ruff errors, 960 pyright errors
Last activity: 2026-02-25 — Plan 11-04 executed (Python sidecar static analysis tooling)

Progress: [███████░░░░░░░░░░░░░] 20% — v1.1 in progress (11-04/~20 total plans)

## Accumulated Context

### Decisions

- [Roadmap]: 5 phases (10-14) derived from 6 requirement categories across 30 requirements
- [Research]: Define standards BEFORE auditing — need a fixed target to measure drift against
- [Research]: Standards documents are inert without paired enforcement (lint rule, tsconfig flag, or agent context injection)
- [Research]: oxlint under-configured — `promise`/`node`/`unicorn` plugins missing; critical rules still at `warn` level
- [Research]: Python sidecar has zero static analysis — ruff + pyright is the correct addition
- [Research]: Pre-commit hook covers only the runtime; UI and sidecar never gated
- [Research]: tsgolint (type-aware oxlint) deferred — alpha, requires TypeScript 7.0 beta
- [Research]: eslint-plugin-svelte in scope — justified exception for template-layer analysis oxlint cannot do
- [Scope]: Agent integration verification (Phase 14) in scope — includes blocking --no-verify in .claude/settings.json
- [Scope]: docs/ARCHITECTURE.md (11 Greek module map) in scope — goes into Phase 10
- [10-01]: auth module has zero aletheia module dependencies by design — only node:crypto and hono
- [10-01]: daemon is a high-layer module — imports nous and distillation for cron/reflection jobs; documented in ARCHITECTURE.md
- [10-01]: dianoia/routes.ts has narrow type import from pylon/routes/deps.ts — accepted architectural exception
- [10-01]: console exception scoped to CLI output functions (nous/audit.ts only), not a blanket ban
- [10-02]: .claude/rules/ files use imperative mood only — rationale prose deliberately excluded (belongs in STANDARDS.md)
- [10-02]: @-import lines placed in Standards section at lines 9-12 — agents encounter rules before Structure/Commands sections
- [10-02]: architecture.md cross-references docs/ARCHITECTURE.md for full dependency table rather than duplicating inline
- [11-01]: Shebang in entry.ts must stay on line 1; eslint-disable placed on line 2 — oxlint honors this position
- [11-01]: StructuredExtractor import was unused cascade from removing extractor field — removed in same commit
- [11-01]: Pre-existing oxlint exit-1 (1 error) confirmed via stash baseline — not introduced by plan changes
- [11-04]: PEP 735 dependency-groups.dev used (uv 0.5+ default) rather than project.optional-dependencies
- [11-04]: pyright typeCheckingMode=strict — establishes intended end state for Phase 13 remediation
- [11-04]: pyright reportMissingTypeStubs=false — mem0ai/qdrant-client/neo4j lack full type stubs
- [11-04]: ruff B008 ignored — FastAPI Depends() is intentional function call in default argument
- [11-04]: Baseline: 152 ruff errors (95 auto-fixable), 960 pyright errors — documented for Phase 12 audit
- [11-03]: @typescript-eslint/parser required for Svelte 5 TS parsing in eslint-plugin-svelte — not in plan, discovered during implementation
- [11-03]: svelte/require-each-key and svelte/prefer-svelte-reactivity downgraded to warn — 70+ violations in existing codebase; Phase 13 remediation target
- [11-03]: eslint v10 flat config dropped --ext flag — file patterns in eslint.config.js replace --ext .svelte
- [11-03]: typecheck --fail-on-warnings intentionally fails until Phase 13; pre-commit hook gates on lint:check only

### Pending Todos

None.

### Blockers/Concerns

- [LOW] `no-restricted-imports` per-module override pattern in oxlint has LOW confidence — verify during Phase 11; fall back to documentation-only if not supported
- [RESOLVED] Actual violation counts unknown until exploratory scan in Phase 10 — now embedded in STANDARDS.md

## Session Continuity

Last session: 2026-02-25
Stopped at: Completed 11-03-PLAN.md (UI oxlint/eslint/tsconfig tooling) — Plan 11-03 complete (out of order; 11-04 was already done)
Resume file: None
