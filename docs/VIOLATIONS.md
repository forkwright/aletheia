# Violations Baseline

**Audit date:** 2026-02-25
**Branch:** main (post-merge with Phase 11 tooling from origin/main)
**Phase 11 tooling:** Confirmed present (promise/prefer-await-to-then in runtime .oxlintrc.json)
**Status:** Phase 12 baseline — do not remediate until Phase 13

---

## Phase 13 Remediation Status

**Completed:** 2026-02-26
**Result:** ALL CLEAN — all three sub-projects pass all lint and typecheck tools with 0 errors and 0 warnings.

| Sub-project | Tool | Before | After |
|-------------|------|--------|-------|
| runtime | oxlint | 590 warnings | 0 warnings |
| runtime | tsc | 0 errors | 0 errors |
| UI | svelte-check | 87 errors, 25 warnings | 0 errors, 0 warnings |
| UI | oxlint | 30 warnings | 0 warnings |
| UI | eslint | 74 warnings | 0 warnings |
| sidecar | ruff | 152 errors | 0 errors |
| sidecar | pyright | 960 errors | 0 errors |

Pre-commit hook updated: `npm run typecheck` added to UI section (STANDARDS.md Known Gap closed).

---

## Summary

| Sub-project | Tool | Errors | Warnings | Auto-fixable | Manual |
|-------------|------|--------|----------|--------------|--------|
| runtime | oxlint | 0 | 590 | 299 | 291 |
| runtime | tsc | 0 | 0 | 0 | 0 |
| UI | svelte-check | 87 | 25 | 0 | 112 |
| UI | oxlint | 0 | 30 | 0 | 30 |
| UI | eslint | 0 | 74 | 0 | 74 |
| sidecar | ruff | 152 | 0 | 95 | 57 |
| sidecar | pyright | 960 | 0 | 0 | 960 |

**Note on pyright 960 errors:** The vast majority (777/960 = 81%) are `reportUnknownVariableType`, `reportUnknownMemberType`, and `reportUnknownArgumentType` — all caused by third-party libraries without type stubs (`mem0`, `qdrant-client`). These require a stub strategy, not mass annotation. See the Sidecar section and Strategic Note for Phase 13 guidance.

**Note on svelte-check 87 errors:** Research estimate was 43 errors. Live count is 87. Use live count — it is ground truth.

**Note on ruff 152 errors:** Research estimate was 30 (Pyflakes-only). Live count is 152. The Phase 11 expanded rule set (B, W, I, N, UP, SIM, TC, RUF) added the difference.

---

## Runtime Violations

### oxlint

590 warnings, 0 errors. All violations are warning-level — no errors (no-console is suppressed in legitimate CLI files via `eslint-disable-next-line` comments in `entry.ts`, `audit.ts`, and `cli.ts`).

| Rule | Severity | Count | Auto-fixable | Phase 13 approach |
|------|----------|-------|--------------|-------------------|
| unicorn/catch-error-name | warn | 224 | YES (`--fix-suggestions`, low risk) | Run with diff review; renames `err`/`e` → `error` in catch blocks |
| require-await | warn | 122 | NO | Use `return Promise.resolve()` in organon/built-in tools; do not remove `async` |
| sort-imports | warn | 67 | YES (`--fix`, safe) | Run directly; member sort within import statement only |
| promise/prefer-await-to-then | warn | 61 | NO | Async refactor per function; structural change required |
| no-explicit-any | warn | 26 | NO | Define types or use `unknown` with narrowing |
| unicorn/no-array-sort | warn | 25 | YES (`--fix-suggestions`, medium risk) | Review diff: `.sort()` → `.toSorted()` returns new array — may break callers holding original ref |
| no-unused-vars | warn | 17 | NO | Remove or use; some may be intentional stubs |
| unicorn/consistent-function-scoping | warn | 13 | NO | Move to correct scope |
| no-shadow | warn | 10 | NO | Rename shadowing variables |
| unicorn/no-array-reverse | warn | 7 | YES (`--fix-suggestions`, medium risk) | Review diff: `.reverse()` → `.toReversed()` |
| promise/always-return | warn | 6 | NO | Add return in `.then()` body |
| no-empty-function | warn | 6 | NO | Document intentional empty functions with comment |
| import/no-named-as-default-member | warn | 2 | NO | Fix import style — access via named export |
| preserve-caught-error | warn | 1 | NO | Use caught error in catch block |
| promise/no-multiple-resolved | warn | 1 | NO | Fix promise that resolves multiple times |
| no-useless-constructor | warn | 1 | NO | Remove empty constructor |
| no-unmodified-loop-condition | warn | 1 | NO | Fix loop condition that never changes |
| **Total** | | **590** | **299 auto-fixable** | |

**Auto-fixable breakdown:**
- `unicorn/catch-error-name` (224) via `--fix-suggestions`
- `sort-imports` (67) via `--fix`
- `unicorn/no-array-sort` (25) via `--fix-suggestions` (review first)
- `unicorn/no-array-reverse` (7) via `--fix-suggestions` (review first)
- Total safe auto-fix: 67 (sort-imports)
- Total review-required auto-fix: 256 (catch-error-name + no-array-sort + no-array-reverse)

### tsc

0 errors. Type checking is clean.

| Error | Count | File | Notes |
|-------|-------|------|-------|
| — | 0 | — | Clean. Enhanced-execution.ts errors fixed in origin/main before Phase 11 PR. |

### Manual Convention Audit

| Rule (STANDARDS.md) | Count | Source | Notes |
|---------------------|-------|--------|-------|
| Typed Errors Only (bare `throw new Error`) | 41 | grep (non-test .ts files) | See file list below; concentrated in dianoia/, semeion/, taxis/, koina/ |
| Event Name Format noun:verb | 16 | grep | All in dianoia module: "approved", "denied", "discussing", "extract", "flush", "notified", "phase-planning", "read", "requirements", "research", "roadmap", "sanitize", "summarize", "test", "verify", "write" |
| No Silent Catch (empty body) | 0 | oxlint no-empty | Zero empty catch blocks detected |
| No Silent Catch (comment-only body) | unknown | manual | Convention body catch requires manual inspection; not mechanically counted |
| Logger Creation Pattern | 0 violations | confirmed | `createLogger` universal in compliant modules |
| No Barrel Files | 0 violations | confirmed | `koina/` has no index.ts by design; `dianoia/index.ts` is intentional module API |
| .js Import Extensions | 0 violations | confirmed | Build fails on extensionless — universally compliant |
| No Floating Promises | 0 violations | oxlint | `typescript/no-floating-promises` at error level; 0 violations |
| Type-Only Imports | 0 violations | oxlint | `typescript/consistent-type-imports` at error level; 0 violations |
| Module Import Direction (no cycles) | 0 violations | oxlint `import/no-cycle` | Zero circular dependencies |

**Bare throw new Error locations (41 total):**

| File | Count |
|------|-------|
| src/koina/encryption.ts | 3 |
| src/nous/recall.ts | 2 |
| src/organon/built-in/ssrf-guard.ts | 2 |
| src/organon/built-in/browser.ts | 1 |
| src/semeion/tts.ts | 4 |
| src/taxis/scaffold.ts | 4 |
| src/portability/import.ts | 1 |
| src/dianoia/execution-tool.ts | 4 |
| src/dianoia/orchestration-core.ts | 1 |
| src/dianoia/orchestrator.ts | 1 |
| src/dianoia/project-files.ts | 2 |
| src/dianoia/research-tool.ts | 1 |
| src/dianoia/standalone/enhanced-execution-tool.ts | 4 |
| src/dianoia/standalone/orchestration-core.ts | 1 |
| src/dianoia/verifier-tool.ts | 6 |
| src/dianoia/execution.ts | 2 |
| src/dianoia/researcher.ts | 1 |
| src/dianoia/roadmap.ts | 1 |

**Non-compliant event names (16 total, all in dianoia):**

`"approved"`, `"denied"`, `"discussing"`, `"extract"`, `"flush"`, `"notified"`, `"phase-planning"`, `"read"`, `"requirements"`, `"research"`, `"roadmap"`, `"sanitize"`, `"summarize"`, `"test"`, `"verify"`, `"write"`

These are dianoia planning pipeline events. Phase 13 should rename to `noun:verb` format (e.g., `"phase:discussing"`, `"plan:extract"`, `"phase:verified"`).

---

## UI Violations

### svelte-check

Live count: 87 errors, 25 warnings (279 files scanned, 35 files with problems).
Research estimate was 43 errors / 22 warnings — live count is ground truth.

| Category | Errors | Warnings | Notes |
|----------|--------|----------|-------|
| Component type errors | 87 | 25 | COMPLETED 279 FILES 87 ERRORS 25 WARNINGS 35 FILES_WITH_PROBLEMS |

All 87 errors and 25 warnings require manual remediation. `svelte-check` has no auto-fix capability.

**Gap explanation:** Research estimate of 43 errors was from pre-Phase-12 baseline. The live count (87) represents current state after Phase 11 tooling landed — additional type strictness from the Phase 11 `tsconfig` changes surfaced more violations in Svelte component boundaries.

### oxlint (UI)

30 warnings, 0 errors.

| Rule | Severity | Count | Auto-fixable | Notes |
|------|----------|-------|--------------|-------|
| typescript-eslint/no-explicit-any | warn | 29 | NO | Replace with specific types or `unknown` with narrowing |
| no-unused-vars | warn | 1 | NO | Remove or use |
| **Total** | | **30** | **0** | All manual |

### eslint (svelte/no-at-html-tags)

74 warnings total across two rules. 0 errors.

| Rule | Count | Files | Auto-fixable | Notes |
|------|-------|-------|--------------|-------|
| svelte/require-each-key | warn | 37 | NO | Add key expression to `{#each}` blocks for performance |
| svelte/prefer-svelte-reactivity | warn | 37 | NO | Replace `Map`/`Set` with `SvelteMap`/`SvelteSet` for reactivity |
| svelte/no-at-html-tags | 0 active | Markdown.svelte, ToolPanel.svelte, FileEditor.svelte, FileExplorer.svelte | N/A | All 4 `{@html}` usages suppressed with `eslint-disable-next-line` + sanitization justification comment |

**`svelte/no-at-html-tags` detail:** All 4 usages are suppressed with inline `eslint-disable-next-line` comments that document the sanitization approach. No active violations — rule is compliant via explicit suppression with justification.

### Convention Audit

| Rule (STANDARDS.md) | Count | Notes |
|---------------------|-------|-------|
| Svelte 5 Runes Only (no legacy reactive syntax) | 0 violations | Confirmed by grep: no `export let` or `$:` patterns found |
| Typed Component Props | covered by svelte-check errors | svelte-check errors include prop type violations |
| No XSS via @html | 0 active violations | All `{@html}` usages suppressed with sanitization justification |
| svelte-check Warnings Are Errors | 25 warnings unresolved | 25 warnings present; CI enforces `--fail-on-warnings` in Phase 11 |

---

## Sidecar Violations

### ruff

152 errors, 0 warnings. 95 auto-fixable with `--fix`; 11 more with `--unsafe-fixes`.
Baseline was 30 (Pyflakes-only). Phase 11 added B, W, I, N, UP, SIM, TC, RUF rule sets — difference (122 additional) is from those new rules.

| Rule Code | Rule Name | Count | Auto-fixable | Description |
|-----------|-----------|-------|--------------|-------------|
| W293 | whitespace-before-newline | 26 | YES (`--fix`) | Trailing whitespace on blank lines |
| B904 | raise-without-from-inside-except | 26 | NO | Missing `raise ... from err` in except blocks |
| UP017 | deprecated-datetime-utc-alias | 21 | YES (`--fix`) | Use `datetime.UTC` instead of `datetime.timezone.utc` |
| I001 | unsorted-imports | 13 | YES (`--fix`) | Import block not sorted (isort) |
| F841 | unused-variable | 13 | YES (`--fix`) | Local variable assigned but never used |
| F401 | unused-import | 10 | YES (`--fix`) | Module imported but unused |
| B033 | duplicate-value | 10 | NO | Set literal with duplicate values |
| N806 | non-lowercase-variable-in-function | 6 | NO | Variable in function should be lowercase (pep8-naming) |
| F541 | f-string-without-placeholders | 6 | YES (`--fix`) | f-string has no format expressions |
| RUF006 | asyncio-dangling-task | 4 | NO | `asyncio.create_task()` result not stored — task may be garbage collected |
| UP045 | non-pep604-optional | 3 | YES (`--fix`) | Use `X \| None` instead of `Optional[X]` |
| RUF002 | ambiguous-unicode-character-docstring | 2 | NO | Ambiguous unicode character in docstring |
| B905 | zip-without-explicit-strict | 2 | NO | `zip()` without `strict=` parameter |
| B007 | unused-loop-control-variable | 2 | NO | Loop control variable not used in loop body |
| SIM114 | if-with-same-arms | 1 | NO | Combine if branches using `or` |
| SIM105 | suppressible-exception | 1 | NO | Use `contextlib.suppress()` instead of `try`/`except`/`pass` |
| SIM103 | needless-bool | 1 | NO | Return condition directly |
| RUF059 | unused-unpacked-variable | 1 | NO | Unpacked variable unused in for loop |
| RUF015 | unnecessary-iterable-allocation-for-first-element | 1 | NO | Use `next(...)` over single-element slice |
| RUF013 | implicit-optional | 1 | NO | Implicit `Optional` prohibited by PEP 484 |
| N803 | argument-lowercase | 1 | NO | Argument name should be lowercase |
| F811 | redefinition-of-unused-name | 1 | NO | Redefinition of unused name from import |
| **Total** | | **152** | **95 auto-fixable** | |

**Auto-fixable breakdown:**
- W293 (26) + UP017 (21) + I001 (13) + F841 (13) + F401 (10) + F541 (6) + UP045 (3) = 92 via `--fix`
- Remaining 3 auto-fixable from minor rules
- 57 require manual remediation

### pyright

960 errors, 0 warnings.

**Strategic note:** 777 of 960 errors (81%) are `reportUnknownVariableType` (319), `reportUnknownMemberType` (293), and `reportUnknownArgumentType` (165). These are caused entirely by third-party libraries (`mem0`, `qdrant-client`) that ship without PEP 561 type stubs. These are NOT project code errors — they are vendor library typing gaps.

**Phase 13 strategy for pyright:** Do NOT attempt mass type annotation of vendor library calls. Instead:
1. Create stub files (`.pyi`) for the narrow public API surface used from `mem0` and `qdrant-client`
2. Or add vendor-specific `# type: ignore[unknown-xxx]` suppressions with a tracking comment
3. Address the remaining 183 genuine project-code errors (reportArgumentType, reportOptionalSubscript, etc.) directly

| Category | Count | Auto-fixable | Origin | Notes |
|----------|-------|--------------|--------|-------|
| reportUnknownVariableType | 319 | NO | vendor libs (mem0, qdrant-client) | Stub strategy required |
| reportUnknownMemberType | 293 | NO | vendor libs (mem0, qdrant-client) | Stub strategy required |
| reportUnknownArgumentType | 165 | NO | vendor libs (mem0, qdrant-client) | Stub strategy required |
| reportUnknownParameterType | 61 | NO | mixed (project + vendor) | Triage needed |
| reportArgumentType | 22 | NO | project code | Genuine type mismatch — fix directly |
| reportOptionalSubscript | 21 | NO | project code | `None` not subscriptable — add None checks |
| reportMissingTypeArgument | 17 | NO | project code | Add generic type arguments |
| reportMissingParameterType | 14 | NO | project code | Add explicit parameter types |
| reportUnusedVariable | 13 | NO | project code | Remove or use |
| reportUnknownLambdaType | 13 | NO | vendor libs | Stub strategy |
| reportUnusedImport | 10 | NO | project code | Remove unused imports |
| reportRedeclaration | 2 | NO | project code | Fix name collision |
| reportCallIssue | 2 | NO | project code | Wrong call signature |
| reportUnusedFunction | 1 | NO | project code | Remove or use |
| reportUnnecessaryIsInstance | 1 | NO | project code | Remove unnecessary isinstance check |
| reportUnnecessaryComparison | 1 | NO | project code | Remove unnecessary comparison |
| reportPrivateUsage | 1 | NO | project code | Don't access private attribute |
| reportPrivateImportUsage | 1 | NO | project code | Private import from library |
| reportOptionalOperand | 1 | NO | project code | Operation on possibly-None value |
| reportOptionalMemberAccess | 1 | NO | project code | Member access on possibly-None |
| reportOperatorIssue | 1 | NO | project code | Operator type mismatch |
| **Total** | **960** | **0** | | **777 vendor stubs, 183 project code** |

---

## Remediation Priority (Phase 13 Backlog)

### P1 — Warnings that block standards compliance (fix first)

These are warning-level violations that STANDARDS.md treats as violations requiring remediation before Phase 13 exits.

| Sub-project | Rule/Check | Count | Approach |
|-------------|-----------|-------|---------|
| runtime | unicorn/catch-error-name | 224 | `oxlint --fix-suggestions` + review diff |
| runtime | require-await | 122 | `return Promise.resolve()` pattern in organon/built-in/; do not remove `async` keyword |
| runtime | sort-imports | 67 | `oxlint --fix` (safe, no review needed) |
| runtime | promise/prefer-await-to-then | 61 | Async refactor per function — structural |
| UI | svelte-check errors | 87 | Component type fixes — manual per component |
| UI | svelte-check warnings | 25 | Address non-error svelte-check warnings |
| sidecar | ruff (auto-fixable) | 95 | `uv run ruff check . --fix` (safe) |
| sidecar | pyright — project code | ~183 | Type narrowing, Optional handling, missing annotations |
| sidecar | pyright — vendor stubs | ~777 | Stub files for mem0 and qdrant-client public API |

### P2 — Important warnings (manual remediation)

| Sub-project | Rule | Count | Approach |
|-------------|------|-------|---------|
| runtime | no-explicit-any | 26 | Define types or use `unknown` + narrowing |
| runtime | unicorn/no-array-sort | 25 | Review + `--fix-suggestions` after review |
| runtime | no-unused-vars | 17 | Remove or use |
| runtime | unicorn/consistent-function-scoping | 13 | Move functions to correct scope |
| runtime | no-shadow | 10 | Rename shadowing variables |
| runtime | unicorn/no-array-reverse | 7 | Review + `--fix-suggestions` after review |
| runtime | promise/always-return | 6 | Add return in `.then()` |
| runtime | no-empty-function | 6 | Document intentional empty functions |
| UI | eslint: svelte/prefer-svelte-reactivity | 37 | Replace `Map`/`Set` with `SvelteMap`/`SvelteSet` |
| UI | eslint: svelte/require-each-key | 37 | Add key expression to `{#each}` blocks |
| UI | oxlint: no-explicit-any | 29 | Replace with specific types or `unknown` |
| sidecar | ruff (manual): B904 | 26 | Add `raise ... from err` in except blocks |
| sidecar | ruff (manual): B033 | 10 | Fix duplicate values in set literals |
| sidecar | ruff (manual): N806/N803 | 7 | Fix variable/argument naming to lowercase |
| sidecar | ruff (manual): RUF006 | 4 | Store asyncio.create_task() result |

### P3 — Convention and minor issues (manual, lower priority)

| Sub-project | Rule | Count | Approach |
|-------------|------|-------|---------|
| runtime | Typed Errors Only (bare throw) | 41 | `AletheiaError` subclass per throw site — domain knowledge required |
| runtime | Event Name Format noun:verb | 16 | Rename dianoia planning events to noun:verb |
| runtime | import/no-named-as-default-member | 2 | Fix import style |
| runtime | preserve-caught-error | 1 | Use caught error in catch block |
| runtime | promise/no-multiple-resolved | 1 | Fix multiple-resolve bug |
| runtime | no-useless-constructor | 1 | Remove empty constructor |
| runtime | no-unmodified-loop-condition | 1 | Fix loop condition |
| sidecar | ruff (manual): B905/B007/SIM*/RUF* | 14 | Per-rule fixes |

---

## Known Exceptions / False Positives

| Rule | Exception | File(s) | Phase 13 handling |
|------|-----------|---------|-------------------|
| no-console | Legitimate CLI stdout output | nous/audit.ts | Already suppressed via `eslint-disable-next-line` |
| no-console | CLI startup messages | entry.ts, cli.ts | Already suppressed via `eslint-disable-next-line` |
| svelte/no-at-html-tags | Markdown rendering (DOMPurify-sanitized) | Markdown.svelte | Suppressed with justification comment — no action needed |
| svelte/no-at-html-tags | Search highlight output (DOMPurify-sanitized) | ToolPanel.svelte, FileExplorer.svelte | Suppressed with justification comment — no action needed |
| svelte/no-at-html-tags | Preview output (HTML-escaped) | FileEditor.svelte | Suppressed with justification comment — no action needed |
| require-await | ToolHandler interface requires `Promise<string>` | organon/built-in/*.ts | Use `Promise.resolve()` pattern — not `eslint-disable` |
| ruff B008 | FastAPI `Depends()` pattern | sidecar routes | Already in `pyproject.toml ignore = ["B008"]` |
| unicorn/no-array-sort | `.sort()` mutates in place; `.toSorted()` does not | See runtime output | Review all 25 before applying `--fix-suggestions` |
| pyright reportUnknown* | Third-party lib without type stubs | All sidecar files calling mem0/qdrant-client | Stub strategy — not mass annotation |

---

## Standards Compliance Summary

Every STANDARDS.md rule is accounted for:

| STANDARDS.md Rule | Status | Count | Section |
|-------------------|--------|-------|---------|
| Typed Errors Only | Partial | 41 bare throws | Runtime — Manual Convention Audit |
| No Silent Catch | Compliant (empty) | 0 | Runtime — Manual Convention Audit |
| No Explicit Any | Violating | 26 runtime + 29 UI | Runtime + UI oxlint |
| Logger Not Console | Compliant (suppressed) | 0 active | Runtime — no-console suppressed in CLI files |
| Typed Promise Returns | Violating | 122 | Runtime oxlint: require-await |
| Sort Named Imports | Violating | 67 | Runtime oxlint: sort-imports |
| Prefer await over .then() | Violating | 61 | Runtime oxlint: promise/prefer-await-to-then |
| Catch param naming | Violating | 224 | Runtime oxlint: unicorn/catch-error-name |
| .js Import Extensions | Compliant | 0 | Runtime — Manual Convention Audit |
| Type-Only Imports | Compliant | 0 | Runtime oxlint: consistent-type-imports |
| No Floating Promises | Compliant | 0 | Runtime oxlint: no-floating-promises |
| No XSS via @html | Compliant (suppressed) | 0 active | UI — svelte/no-at-html-tags with justification |
| Svelte 5 Runes Only | Compliant | 0 | UI — Convention Audit |
| svelte-check Warnings | Violating | 87 errors + 25 warnings | UI — svelte-check |
| Typed Component Props | Violating (covered by svelte-check) | 87 | UI — svelte-check |
| FastAPI Depends() | Compliant | 0 (B008 ignored) | Sidecar — ruff pyproject.toml |
| Ruff Rule Set | Violating | 152 | Sidecar — ruff |
| Pyright Strict Mode | Violating | 960 | Sidecar — pyright |
| No Bare Exception Catch | Violating | 26 (B904) | Sidecar — ruff |
| Gnomon Naming | Compliant | 0 | Convention |
| Module Import Direction | Compliant | 0 | Runtime oxlint: import/no-cycle |
| Event Name Format noun:verb | Violating | 16 (dianoia) | Runtime — Manual Convention Audit |
| Logger Creation Pattern | Compliant | 0 | Convention |
| No Barrel Files | Compliant | 0 | Convention |
| One Module Per PR | Process rule | N/A | Phase 13 execution plan |
