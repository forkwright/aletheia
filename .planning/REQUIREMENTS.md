# Requirements: Aletheia v1.1 Standards & Hardening

**Defined:** 2026-02-25
**Core Value:** A personal AI runtime you fully control — persistent across sessions, extensible through modules, and powerful enough to handle complex multi-agent work without losing context.

## v1.1 Requirements

Standards are only real when paired with enforcement. This milestone converts aspirational standards into operational ones across all three Aletheia sub-projects (TypeScript runtime, Svelte 5 UI, Python FastAPI memory sidecar). No new features — refactor only, bugs fixed as found.

### Standards Documentation

- [x] **STDS-01**: `docs/STANDARDS.md` exists with opinionated, rationale-documented rules for TypeScript, Svelte, Python, and architecture — each rule states what, why, compliant example, non-compliant example, and which tool enforces it
- [x] **STDS-02**: `docs/ARCHITECTURE.md` exists with the Aletheia module map (11 Greek modules), initialization order, directed dependency graph, and extension points — currently undocumented outside code comments
- [x] **STDS-03**: Every rule in `docs/STANDARDS.md` has either a paired mechanical enforcement path (lint rule, tsconfig flag) or an explicit "enforced by convention + agent context" annotation — no rule exists without knowing how it is enforced
- [x] **STDS-04**: An exploratory standards scan runs before standards are finalized, so violation counts are evidence-driven rather than ideological

### Tooling Configuration

- [x] **TOOL-01**: `infrastructure/runtime/.oxlintrc.json` hardened: `promise`, `node`, `unicorn` plugins enabled; `no-unused-vars`, `no-empty-function`, `require-await`, `typescript/no-explicit-any` promoted from `warn` to `error`; `import/no-cycle`, `no-console`, `suspicious` category added
- [x] **TOOL-02**: `infrastructure/runtime/tsconfig.json` updated with `noImplicitReturns` and `noImplicitOverride` — the two missing strict flags that pair with existing `noFallthroughCasesInSwitch` and `override` usage
- [x] **TOOL-03**: `ui/.oxlintrc.json` created with a UI-appropriate rule subset — inherits runtime philosophy, relaxes rules that don't apply in Svelte event handler context
- [x] **TOOL-04**: `ui/tsconfig.json` updated with parity flags: `noUnusedLocals`, `noUnusedParameters`, `noImplicitReturns`, `noImplicitOverride`, `noFallthroughCasesInSwitch`, `exactOptionalPropertyTypes`
- [x] **TOOL-05**: `svelte-check --fail-on-warnings` enabled in the UI typecheck script — Svelte compiler warnings become blocking errors
- [x] **TOOL-06**: `eslint-plugin-svelte` installed and configured for the UI — provides template-layer analysis (XSS prevention, rune-specific rules) that oxlint cannot cover due to its Svelte 5 template limitation
- [x] **TOOL-07**: Python sidecar gains `ruff` and `pyright` as dev dependencies — `uv add --dev ruff pyright` — with `[tool.ruff]` and `[tool.pyright]` sections added to `infrastructure/memory/sidecar/pyproject.toml`
- [ ] **TOOL-08**: All final lint configs contain zero `"warn"` rules — every rule is `"error"` or absent. The single exception: `suspicious` category starts as `"warn"` and is promoted to `"error"` after Phase 3 audit confirms zero violations

### Pre-commit Hook Coverage

- [ ] **HOOK-01**: `.githooks/pre-commit` extended to cover the UI sub-project — `svelte-check`, `tsc --noEmit`, and `oxlint` run for UI when `.svelte` or `ui/` files are staged
- [ ] **HOOK-02**: `.githooks/pre-commit` extended to cover the Python sidecar — `ruff check` and `pyright` run for sidecar when `infrastructure/memory/sidecar/` files are staged
- [ ] **HOOK-03**: Pre-commit hook uses conditional checks (run only when relevant files are staged) — total hook execution time stays under 10 seconds for any single sub-project change
- [ ] **HOOK-04**: Hook is documented in `docs/STANDARDS.md` — explains what runs, when, and how to install (`git config core.hooksPath .githooks`)

### Audit

- [ ] **AUDT-01**: Hardened toolchain runs against all three sub-projects and produces a baseline violation count by sub-project and by category (correctness bugs / pattern violations / architecture violations)
- [ ] **AUDT-02**: `import/no-cycle` scan produces a baseline circular dependency report — violations categorized as intentional (event bus publish/subscribe patterns) vs. accidental
- [ ] **AUDT-03**: Audit output is triaged before any remediation begins — Tier 1 (correctness bugs, fix immediately), Tier 2 (violations in high-churn files), Tier 3 (violations in stable files, batch by module)
- [ ] **AUDT-04**: Manual standards audit covers what tooling cannot check: error class hierarchy usage (AletheiaError subclass required), event name format (`noun:verb`), logger creation (`createLogger("module-name")`), import order compliance

### Remediation

- [ ] **RMED-01**: All Tier 1 violations (correctness bugs found during audit) are fixed — each fix is a targeted single-module PR with scoped vitest run after
- [ ] **RMED-02**: All Tier 2 and Tier 3 pattern violations in the runtime are remediated — one module per PR, `npx vitest run src/[module]/` after each batch
- [ ] **RMED-03**: All violations in the UI sub-project are remediated — `svelte-check --fail-on-warnings` and `eslint-plugin-svelte` both pass clean
- [ ] **RMED-04**: All violations in the Python sidecar are remediated — `ruff check` and `pyright` both pass clean
- [ ] **RMED-05**: Full integration test suite passes clean after all remediation is complete — `npx vitest run -c vitest.integration.config.ts`
- [ ] **RMED-06**: `suspicious` oxlint category promoted from `"warn"` to `"error"` once the codebase is clean

### Agent Integration

- [x] **AGNT-01**: `.claude/rules/` directory created with four per-language agent-action files: `typescript.md`, `svelte.md`, `python.md`, `architecture.md` — each under 200 lines, imperative mood, with correct/incorrect code examples
- [x] **AGNT-02**: `CLAUDE.md` updated with `@`-imports for the four `.claude/rules/` files — agents dispatched via `sessions_dispatch` automatically receive the standards in their context
- [ ] **AGNT-03**: `.claude/settings.json` updated to block `git commit --no-verify` — prevents agents from bypassing pre-commit hooks when lint rules create friction
- [ ] **AGNT-04**: A test agent dispatched via `sessions_dispatch` on a small coding task follows the error class hierarchy, import conventions, naming patterns, and logger usage without any explicit standards instruction in the task description — verified through code review of the output

---

## v2 Requirements

Deferred. Tracked but not in current roadmap.

### Advanced Agent Tooling

- **AGNT-V2-01**: `standards_lookup` skill — query-based agent retrieval of standards sections; build after `docs/STANDARDS.md` is proven insufficient as direct Read context
- **AGNT-V2-02**: Standards freshness automation — alert when lint rule configs diverge from `docs/STANDARDS.md`; manual protocol first, script second

### Module Boundary Enforcement

- **MODB-V2-01**: Module boundary enforcement tooling (`eslint-plugin-boundaries` or similar) — document-first approach in v1.1 via `docs/ARCHITECTURE.md`; automate only if violations prove persistent after documentation
- **MODB-V2-02**: Oxlint `no-restricted-imports` per-module override pattern for directional boundary enforcement — LOW confidence in current oxlint support; validate in Phase 2, promote to v2 tooling if confirmed working

### Type-Aware Linting

- **LINT-V2-01**: `oxlint-tsgolint` type-aware rules (`no-floating-promises`, `no-misused-promises`) — deferred until stable mid-2026; requires TypeScript 7.0 and `tsgolint` GA

---

## Out of Scope

| Feature | Reason |
|---------|--------|
| Prettier or dprint | Formatting churn with no net benefit; oxlint handles style rules; existing code is already consistent |
| ESLint alongside oxlint for the runtime | 30x slowdown on type-aware rules; conflicting configs; oxlint covers this surface |
| lint-staged | Cannot scope TypeScript type checks to staged files correctly; conditional checks in the hook are sufficient |
| Husky | `.githooks/pre-commit` already works; replacing it with Husky adds a dependency with no functional benefit |
| 100% test coverage mandate | Gaming risk; behavior-not-implementation testing philosophy conflicts with line coverage metrics |
| Barrel files (`index.ts` re-exports) | Circular dependency risk, tree-shaking failures, IDE navigation noise; already correctly avoided in codebase |
| Monorepo tooling (Nx, Turborepo) | 3 sub-projects with independent toolchains is not a monorepo scaling problem |
| CHANGELOG | Personal tool with a single maintainer; git log + spec docs serve this purpose |
| ADR format | Aletheia's numbered spec docs in `docs/specs/` already serve this role |
| `ANN` or `D` ruff rules | FastAPI/Pydantic v2 handles return type annotations; docstring convention is self-documenting names |

---

## Traceability

Phase numbering continues from v1.0 (Phases 1–9 complete).

| Requirement | Phase | Status |
|-------------|-------|--------|
| STDS-01 | Phase 10 | Complete |
| STDS-02 | Phase 10 | Complete |
| STDS-03 | Phase 10 | Complete |
| STDS-04 | Phase 10 | Complete |
| TOOL-01 | Phase 11 | Complete |
| TOOL-02 | Phase 11 | Complete |
| TOOL-03 | Phase 11 | Complete |
| TOOL-04 | Phase 11 | Complete |
| TOOL-05 | Phase 11 | Complete |
| TOOL-06 | Phase 11 | Complete |
| TOOL-07 | Phase 11 | Complete |
| TOOL-08 | Phase 14 | Pending |
| HOOK-01 | Phase 11 | Pending |
| HOOK-02 | Phase 11 | Pending |
| HOOK-03 | Phase 11 | Pending |
| HOOK-04 | Phase 11 | Pending |
| AUDT-01 | Phase 12 | Pending |
| AUDT-02 | Phase 12 | Pending |
| AUDT-03 | Phase 12 | Pending |
| AUDT-04 | Phase 12 | Pending |
| RMED-01 | Phase 13 | Pending |
| RMED-02 | Phase 13 | Pending |
| RMED-03 | Phase 13 | Pending |
| RMED-04 | Phase 13 | Pending |
| RMED-05 | Phase 13 | Pending |
| RMED-06 | Phase 14 | Pending |
| AGNT-01 | Phase 10 | Complete |
| AGNT-02 | Phase 10 | Complete |
| AGNT-03 | Phase 14 | Pending |
| AGNT-04 | Phase 14 | Pending |

**Coverage:**
- v1.1 requirements: 30 total
- Mapped to phases: 30
- Unmapped: 0 ✓

**Phase summary:**
- Phase 10: Standards Definition + Agent Context Infrastructure (STDS-01–04, AGNT-01–02)
- Phase 11: Tooling Configuration + Pre-commit Coverage (TOOL-01–07, HOOK-01–04)
- Phase 12: Audit (AUDT-01–04)
- Phase 13: Remediation (RMED-01–05)
- Phase 14: Polish — warn→error promotion, agent verification, --no-verify block (TOOL-08, RMED-06, AGNT-03–04)

---
*Requirements defined: 2026-02-25*
*Last updated: 2026-02-25 after initial v1.1 definition*
