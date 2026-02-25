# Roadmap: Aletheia v1.1 Standards & Hardening

## Overview

v1.1 converts aspirational standards into operational ones across all three Aletheia sub-projects (TypeScript runtime, Svelte 5 UI, Python FastAPI memory sidecar). No new features — refactor only, bugs fixed as found. The build order is non-negotiable: define the standard before configuring tooling, configure tooling before auditing, audit before remediating.

Phase numbering continues from v1.0 (Phases 1–9 complete).

## Milestones

### v1.0 Dianoia MVP ✅

See `.planning/milestones/v1.0-ROADMAP.md` for full details (9 phases, 29 plans, shipped 2026-02-25).

---

## v1.1 Phases

- [x] **Phase 10: Standards Definition + Agent Context Infrastructure** (completed 2026-02-25)
- [ ] **Phase 11: Tooling Configuration + Pre-commit Coverage**
- [ ] **Phase 12: Audit**
- [ ] **Phase 13: Remediation**
- [ ] **Phase 14: Polish**

---

## Phase Details

### Phase 10: Standards Definition + Agent Context Infrastructure

**Goal:** A written, rationale-documented standard exists as the fixed target for all subsequent tooling and audit work — and agents dispatched via `sessions_dispatch` automatically receive the standards in their context.

**Depends on:** Nothing (first v1.1 phase)

**Requirements:** STDS-01, STDS-02, STDS-03, STDS-04, AGNT-01, AGNT-02

**Success Criteria** (what must be TRUE):
1. `docs/STANDARDS.md` exists with documented rules for TypeScript, Svelte, Python, and architecture — each rule includes what/why/compliant/non-compliant/enforced-by
2. `docs/ARCHITECTURE.md` exists with the module map (11 Greek modules), directed dependency graph, initialization order, and extension points
3. `.claude/rules/typescript.md`, `svelte.md`, `python.md`, `architecture.md` exist — each under 200 lines, imperative mood, with code examples
4. `CLAUDE.md` updated with `@`-imports for the four `.claude/rules/` files
5. An exploratory scan has been run and violation counts are referenced in STANDARDS.md (standards are evidence-driven, not ideological)
6. Every rule in STANDARDS.md has either a mechanical enforcement path or an explicit "enforced by convention + agent context" annotation

**Plans:** 2/2 plans complete

Plans:
- [ ] 10-01-PLAN.md — Author docs/STANDARDS.md (with live scan evidence) and docs/ARCHITECTURE.md (all 14 modules + verified dependency graph)
- [ ] 10-02-PLAN.md — Create .claude/rules/ directory with four agent-action files and update CLAUDE.md with @-import chain

---

### Phase 11: Tooling Configuration + Pre-commit Coverage

**Goal:** The defined standards are enforced by tooling across all three sub-projects, and the pre-commit hook gates all of them.

**Depends on:** Phase 10 (need the defined standard to configure tooling against)

**Requirements:** TOOL-01, TOOL-02, TOOL-03, TOOL-04, TOOL-05, TOOL-06, TOOL-07, HOOK-01, HOOK-02, HOOK-03, HOOK-04

**Success Criteria** (what must be TRUE):
1. Runtime oxlint config: `promise`/`node`/`unicorn` plugins enabled; critical rules promoted from `"warn"` to `"error"`; `import/no-cycle` and `no-console` added
2. Runtime tsconfig: `noImplicitReturns` and `noImplicitOverride` added — `npx tsc --noEmit` clean
3. UI oxlint config: `ui/.oxlintrc.json` created with UI-appropriate rule subset; `svelte-check --fail-on-warnings` in typecheck script
4. UI tsconfig: parity flags added (`noUnusedLocals`, `noUnusedParameters`, `noImplicitReturns`, `noImplicitOverride`, `noFallthroughCasesInSwitch`, `exactOptionalPropertyTypes`)
5. `eslint-plugin-svelte` installed and configured for the UI — template-layer analysis active
6. Python sidecar: `ruff` and `pyright` installed as dev dependencies; `[tool.ruff]` and `[tool.pyright]` sections in `pyproject.toml`
7. Pre-commit hook covers all three sub-projects with conditional checks — total hook time under 10 seconds per sub-project change

**Plans:** 3/5 plans executed

Plans:
- [ ] 11-01-PLAN.md — Fix enhanced-execution.ts tsc errors + add no-console disable comments to exception files (runtime pre-conditions)
- [ ] 11-02-PLAN.md — Harden infrastructure/runtime/.oxlintrc.json + add noImplicitReturns/noImplicitOverride to tsconfig
- [ ] 11-03-PLAN.md — Create ui/.oxlintrc.json + tsconfig parity flags + install eslint-plugin-svelte + fail-on-warnings
- [ ] 11-04-PLAN.md — Install ruff + pyright in Python sidecar + configure pyproject.toml sections
- [ ] 11-05-PLAN.md — Extend pre-commit hook to all three sub-projects (conditional) + document in STANDARDS.md

---

### Phase 12: Audit

**Goal:** A triaged violation report exists that tells us exactly what the codebase owes against the defined standard — with bugs separated from style violations and high-churn from stable files.

**Depends on:** Phase 10 (need the defined standard), Phase 11 (need the hardened toolchain)

**Requirements:** AUDT-01, AUDT-02, AUDT-03, AUDT-04

**Success Criteria** (what must be TRUE):
1. Baseline violation counts exist for all three sub-projects, broken down by category (correctness / pattern / architecture)
2. `import/no-cycle` report exists with violations categorized as intentional vs. accidental
3. Triage is complete — Tier 1 (correctness bugs), Tier 2 (high-churn file violations), Tier 3 (stable file violations, batch by module) — before any remediation begins
4. Manual audit covers what tooling cannot: error class hierarchy usage, event name format, logger creation, import order
5. The audit report is committed to `.planning/` — not started until all of Phase 11 is complete

**Plans:** TBD

---

### Phase 13: Remediation

**Goal:** The codebase passes all hardened lint checks clean across all three sub-projects, with bugs fixed and all violations remediated.

**Depends on:** Phase 12 (need the triaged audit report)

**Requirements:** RMED-01, RMED-02, RMED-03, RMED-04, RMED-05

**Success Criteria** (what must be TRUE):
1. All Tier 1 violations (correctness bugs) fixed — each in its own targeted single-module PR
2. Runtime `npx oxlint src/` clean with zero errors
3. UI `eslint-plugin-svelte` and `svelte-check --fail-on-warnings` both pass clean
4. Python sidecar `ruff check` and `pyright` both pass clean
5. Full integration test suite passes clean: `npx vitest run -c vitest.integration.config.ts`
6. No cross-module bulk fix PRs — each batch is scoped to one module, with `npx vitest run src/[module]/` run after each

**Plans:** TBD

---

### Phase 14: Polish

**Goal:** Standards hardening is complete — warn→error promotion done, agent bypass blocked, agent behavior verified.

**Depends on:** Phase 13 (codebase must be clean before promoting warns to errors)

**Requirements:** TOOL-08, RMED-06, AGNT-03, AGNT-04

**Success Criteria** (what must be TRUE):
1. `suspicious` oxlint category promoted from `"warn"` to `"error"` — final lint config contains zero `"warn"` rules
2. `.claude/settings.json` updated to block `git commit --no-verify`
3. A test agent dispatched via `sessions_dispatch` produces code that follows error class hierarchy, import conventions, naming patterns, and logger usage without explicit standards instruction in the task description
4. `CONTRIBUTING.md` updated to cross-reference `docs/STANDARDS.md` and note the hook installation step
5. `npx tsc --noEmit` clean, `npx oxlint src/` clean, full vitest suite passes

**Plans:** TBD

---

## Progress

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 10. Standards Definition + Agent Context | 2/2 | Complete    | 2026-02-25 |
| 11. Tooling Configuration + Pre-commit | 3/5 | In Progress|  |
| 12. Audit | 0/TBD | Not started | — |
| 13. Remediation | 0/TBD | Not started | — |
| 14. Polish | 0/TBD | Not started | — |

---
*Roadmap created: 2026-02-25*
*Last updated: 2026-02-25 — Phase 11 planned (5 plans, 3 waves)*
