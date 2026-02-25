---
phase: 11-tooling-configuration-pre-commit-coverage
plan: "03"
subsystem: ui
tags: [eslint, oxlint, svelte, typescript, static-analysis, linting, eslint-plugin-svelte]

requires:
  - phase: 11-01
    provides: tsc baseline clean — pre-condition for UI tooling configuration

provides:
  - ui/.oxlintrc.json with typescript/import/unicorn plugins, correctness:error category
  - ui/tsconfig.json with all 6 parity flags: noUnusedLocals, noUnusedParameters, noImplicitReturns, noImplicitOverride, noFallthroughCasesInSwitch, exactOptionalPropertyTypes
  - eslint-plugin-svelte 3.15.0 installed with @typescript-eslint/parser for Svelte 5 TypeScript script blocks
  - ui/eslint.config.js with flat/recommended + TypeScript sub-parser for .svelte and .svelte.ts files
  - npm run lint:check runs both oxlint and eslint, exits 0 (0 errors, 104 warnings total)

affects: [11-05, 13-ui-remediation]

tech-stack:
  added: [eslint==10.0.2, eslint-plugin-svelte==3.15.0, globals==17.3.0, "@typescript-eslint/parser"==8.56.1]
  patterns: [ESLint v9/v10 flat config for Svelte 5, TypeScript sub-parser in svelte-eslint-parser parserOptions]

key-files:
  created: [ui/.oxlintrc.json, ui/eslint.config.js]
  modified: [ui/tsconfig.json, ui/package.json, ui/src/components/chat/ChatView.svelte, ui/src/components/chat/Markdown.svelte, ui/src/components/chat/ToolPanel.svelte, ui/src/components/files/FileEditor.svelte, ui/src/components/files/FileExplorer.svelte, ui/src/components/graph/Graph2D.svelte, ui/src/components/graph/Graph3D.svelte, ui/src/components/graph/GraphView.svelte, ui/src/components/planning/api.ts]

key-decisions:
  - "eslint-plugin-svelte flat/recommended requires @typescript-eslint/parser as parserOptions.parser — without it, TypeScript syntax in script blocks fails to parse (oxlint does not have this requirement)"
  - "svelte/require-each-key and svelte/prefer-svelte-reactivity downgraded to warn — 40+ violations widespread in existing components; Phase 13 remediation target"
  - "svelte/no-at-html-tags kept at error; all 4 {@html} usages have eslint-disable comments documenting DOMPurify or HTML-escape sanitization"
  - "@typescript-eslint/parser added as separate devDependency for .svelte.ts file parsing (svelte-eslint-parser covers .svelte files with tsParser as sub-parser)"
  - "lint:check uses eslint src/ without --ext .svelte (deprecated in ESLint v10 flat config); file targeting is handled by eslint.config.js patterns"
  - "typecheck --fail-on-warnings added; command will fail until Phase 13 clears existing svelte-check errors — hook gates on lint:check only, not typecheck"

patterns-established:
  - "ESLint v10 flat config with eslint-plugin-svelte: import tsParser and set parserOptions.parser for both .svelte and .svelte.ts configs"
  - "oxlint-disable-next-line no-unassigned-vars for Svelte bind:this pattern — assigned by runtime, not detected by static analysis"

requirements-completed: [TOOL-03, TOOL-04, TOOL-05, TOOL-06]

duration: 25min
completed: 2026-02-25
---

# Phase 11 Plan 03: UI Tooling Configuration Summary

**UI tooling baseline established: ui/.oxlintrc.json with UI-appropriate rule subset, 6 tsconfig parity flags, and eslint-plugin-svelte flat config wired for Svelte 5 TypeScript scripts; npm run lint:check exits 0**

## Performance

- **Duration:** ~25 min
- **Started:** 2026-02-25T17:29:00Z
- **Completed:** 2026-02-25T17:54:00Z
- **Tasks:** 2
- **Files modified:** 11

## Accomplishments

- Created `ui/.oxlintrc.json` with typescript/import/unicorn plugins, correctness:error, UI-appropriate rule subset per plan rationale (no-console excluded, no-explicit-any at warn, etc.)
- Added 6 parity flags to `ui/tsconfig.json`: noUnusedLocals, noUnusedParameters, noImplicitReturns, noImplicitOverride, noFallthroughCasesInSwitch, exactOptionalPropertyTypes
- Installed eslint 10.0.2, eslint-plugin-svelte 3.15.0, globals 17.3.0, @typescript-eslint/parser 8.56.1 as devDependencies
- Created `ui/eslint.config.js` with flat/recommended config, TypeScript sub-parser wired for Svelte 5 script blocks, svelte/no-at-html-tags and svelte/no-target-blank as error
- Updated `ui/package.json` scripts: lint:check now runs both oxlint and eslint; typecheck now has --fail-on-warnings
- `npm run lint:check` exits 0 (30 oxlint warnings, 74 eslint warnings, 0 errors total)

## Task Commits

Each task was committed atomically:

1. **Task 1: Create ui/.oxlintrc.json and update tsconfig.json with parity flags** - `9ad5219` (feat)
2. **Task 2: Install eslint-plugin-svelte and create ui/eslint.config.js** - `640d497` (feat)

**Plan metadata:** (docs commit follows)

## Files Created/Modified

- `ui/.oxlintrc.json` — Created with typescript/import/unicorn plugins, correctness:error, UI rule subset
- `ui/eslint.config.js` — Created with eslint-plugin-svelte flat/recommended, TypeScript parser, svelte/no-at-html-tags and svelte/no-target-blank as error, warn-level overrides for Phase 13 items
- `ui/tsconfig.json` — Added 6 parity flags (noUnusedLocals, noUnusedParameters, noImplicitReturns, noImplicitOverride, noFallthroughCasesInSwitch, exactOptionalPropertyTypes)
- `ui/package.json` — Updated lint, lint:check, typecheck scripts; added eslint/eslint-plugin-svelte/globals/@typescript-eslint/parser devDeps
- `ui/src/components/chat/ChatView.svelte` — Merged 3 duplicate `../../lib/types` imports (import/no-duplicates)
- `ui/src/components/chat/Markdown.svelte` — Added eslint-disable comment on `{@html}` (DOMPurify-sanitized)
- `ui/src/components/chat/ToolPanel.svelte` — Added eslint-disable comment on `{@html}` (DOMPurify-sanitized)
- `ui/src/components/files/FileEditor.svelte` — Added oxlint-disable for bind:this; added eslint-disable on `{@html}` (HTML-escaped)
- `ui/src/components/files/FileExplorer.svelte` — Added eslint-disable comment on `{@html}` (DOMPurify-sanitized)
- `ui/src/components/graph/Graph2D.svelte` — oxlint-disable for bind:this; eqeqeq fix (`!= null` → `!== null`)
- `ui/src/components/graph/Graph3D.svelte` — oxlint-disable for bind:this; eqeqeq fix (`== null` → `=== null || === undefined`)
- `ui/src/components/graph/GraphView.svelte` — oxlint-disable for consistent-type-imports false positive (Graph2D used as value in template, not visible to script-only analysis)
- `ui/src/components/planning/api.ts` — Removed useless `?? {}` fallback in spread (no-useless-fallback-in-spread)

## Decisions Made

- **@typescript-eslint/parser required for Svelte 5 TS parsing:** eslint-plugin-svelte's svelte-eslint-parser cannot parse TypeScript syntax in `<script lang="ts">` blocks without a TypeScript sub-parser. This was NOT documented in the plan and required discovery and installation.
- **svelte/require-each-key at warn:** 30+ `{#each}` blocks without keys across existing components. Adding keys requires knowing the unique identifier for each collection — this is Phase 13 remediation work, not tooling configuration.
- **svelte/prefer-svelte-reactivity at warn:** 20+ Map/Set/Date instances in stores and components that need migration to SvelteMap/SvelteSet/SvelteDate from `svelte/reactivity`. Phase 13 remediation target.
- **--ext .svelte deprecated in ESLint v10:** The plan specified `eslint src/ --ext .svelte` but ESLint v10 removed `--ext` support in flat config mode. The command is simply `eslint src/` — file targeting is in the config.

## Baseline Violation Counts (Phase 13 Audit Reference)

### oxlint warnings: 30

All at warn level — `no-explicit-any` (28), `no-unused-vars` (1 GitFileStatus in stores), `no-unassigned-vars` (suppressed for bind:this patterns)

### eslint warnings: 74

- `svelte/require-each-key`: ~40 violations across components
- `svelte/prefer-svelte-reactivity`: ~30 violations (Map/Set/Date → SvelteMap/SvelteSet/SvelteDate)
- Both downgraded to warn for Phase 13 remediation

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] eqeqeq violations — `== null` comparisons**
- **Found during:** Task 1 (when new oxlint config activated eqeqeq rule)
- **Issue:** Graph2D.svelte line 70 (`!= null`), Graph3D.svelte lines 87-88 (`== null`)
- **Fix:** Changed to `!== null` and `=== null || === undefined` respectively
- **Files modified:** `ui/src/components/graph/Graph2D.svelte`, `ui/src/components/graph/Graph3D.svelte`
- **Commit:** `9ad5219`

**2. [Rule 1 - Bug] import/no-duplicates — three separate `../../lib/types` imports in ChatView.svelte**
- **Found during:** Task 1 (import/no-duplicates error with new config)
- **Issue:** ToolCallState, MediaItem, CommandInfo imported from same module in 3 separate import statements
- **Fix:** Merged into single `import type { ToolCallState, MediaItem, CommandInfo } from "../../lib/types"`
- **Files modified:** `ui/src/components/chat/ChatView.svelte`
- **Commit:** `9ad5219`

**3. [Rule 2 - Missing functionality] @typescript-eslint/parser not in plan**
- **Found during:** Task 2 (eslint parse errors on .svelte files with TypeScript)
- **Issue:** Plan did not include @typescript-eslint/parser in the install list; svelte-eslint-parser requires it for TypeScript syntax in script blocks
- **Fix:** Added `npm install --save-dev @typescript-eslint/parser` and wired `parser: tsParser` in parserOptions
- **Files modified:** `ui/eslint.config.js`, `ui/package.json`
- **Commit:** `640d497`

**4. [Rule 1 - Bug] eslint v10 dropped --ext flag**
- **Found during:** Task 2 (lint:check failed with ESLint deprecation)
- **Issue:** Plan specified `eslint src/ --ext .svelte` but ESLint v10 removed --ext in flat config mode
- **Fix:** Changed to `eslint src/` — file patterns are in eslint.config.js
- **Files modified:** `ui/package.json`
- **Commit:** `640d497`

**5. [Rule 2 - Missing scope] svelte/require-each-key and svelte/prefer-svelte-reactivity**
- **Found during:** Task 2 (eslint errors on 50+ existing violations)
- **Issue:** flat/recommended includes these as error-level; 40+ and 30+ violations in existing codebase
- **Fix:** Downgraded to warn for both .svelte and .svelte.ts files; documented for Phase 13 remediation
- **Files modified:** `ui/eslint.config.js`
- **Commit:** `640d497`

## Issues Encountered

None requiring user action.

## User Setup Required

None.

## Next Phase Readiness

- Plan 11-05 (pre-commit hook) can gate on `npm run lint:check` in ui/ — exits 0
- Plan 11-05 should NOT gate on `npm run typecheck` — fails until Phase 13 clears svelte-check errors
- Phase 13 has baseline warning counts: ~40 require-each-key, ~30 prefer-svelte-reactivity, 28 no-explicit-any

## Self-Check: PASSED

All claimed files and commits verified present.

---
*Phase: 11-tooling-configuration-pre-commit-coverage*
*Completed: 2026-02-25*
