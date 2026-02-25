# Spec 33: Gnomon Alignment — Module Identity and Naming Infrastructure

**Status:** Draft
**Author:** Syn
**Date:** 2026-02-25
**Spec:** 33

---

## Problem

The gnomon naming system (`docs/gnomon.md`) establishes a principled approach to naming: modules are named for *modes of attention*, not implementations. The system works at four layers (practical → structural → philosophical → reflexive) and demands topological coherence across the whole.

The codebase is partially aligned. Some modules already carry gnomon names (dianoia, hermeneus, mneme, nous, organon, prostheke, pylon, semeion, taxis, koina). Two more were just renamed (auth → symbolon, distillation → melete in PR #225). But three problems remain:

### 1. Remaining module renames

`portability/` still names an outcome rather than a mode of attention. The sub-agent roles (`coder`, `reviewer`, `researcher`, `explorer`, `runner`) name job functions rather than epistemic stances. Workspace files (`GOALS.md`, `MEMORY.md`) use flat English labels in a system where the storage layer is already named `mneme` — topological inconsistency.

### 2. Names are not variables

Renaming `auth` → `symbolon` touched 49 files because module identity is scattered across import paths, logger prefixes, error module strings, and event names as raw string literals. Every future gnomon rename will repeat this O(n) cost unless the architecture supports O(1) identity changes.

### 3. No module boundary discipline

Consumers import directly into module internals (`../symbolon/middleware.js`). There are no barrel exports, no public surface declarations. This means:
- Internal reorganization ripples outward
- The dependency graph is tangled at the file level rather than the module level
- Module encapsulation exists by convention only

---

## Design

### Principles

1. **Define once, propagate everywhere.** A module's identity (name, logger prefix, error module, route prefix, event namespace) is declared exactly once at the module boundary. Everything else references the declaration.

2. **Metaxy context propagation.** Same principle as theke's metaxy: push context down only as far as needed. A consumer imports from the module boundary; it never reaches into internals. Each layer receives only what it needs.

3. **Renames are O(1).** After this spec, renaming a module means: rename the directory, update one tsconfig path alias, update the module's `meta.ts`. That's it. No grep-and-replace across dozens of files.

4. **Topology before labels.** The triad SOUL.md + TELOS.md + MNEME.md is topologically coherent in a way SOUL + GOALS + MEMORY is not. Names compose across layers — workspace files, runtime modules, and documentation should use the same vocabulary.

5. **Recognition, not disruption.** Renames happen in the codebase; API routes and external contracts stay stable. `/api/auth/*` doesn't become `/api/symbolon/*` — the HTTP route is a public contract, not a module name.

---

## Phases

### Phase 1: Module Boundary Infrastructure

**Scope:** tsconfig path aliases + barrel exports for all gnomon modules. This is the prerequisite that makes all subsequent renames cheap.

**Changes:**

- `infrastructure/runtime/tsconfig.json` — Add path aliases:
  ```jsonc
  {
    "compilerOptions": {
      "paths": {
        "@symbolon/*": ["src/symbolon/*"],
        "@melete/*": ["src/melete/*"],
        "@hermeneus/*": ["src/hermeneus/*"],
        "@pylon/*": ["src/pylon/*"],
        "@dianoia/*": ["src/dianoia/*"],
        "@nous/*": ["src/nous/*"],
        "@mneme/*": ["src/mneme/*"],
        "@organon/*": ["src/organon/*"],
        "@prostheke/*": ["src/prostheke/*"],
        "@semeion/*": ["src/semeion/*"],
        "@taxis/*": ["src/taxis/*"],
        "@koina/*": ["src/koina/*"],
        "@daemon/*": ["src/daemon/*"]
      }
    }
  }
  ```
  Note: `portability` is excluded — it will be renamed in Phase 3.

- `src/*/index.ts` — Create barrel exports for each module declaring the public API surface. Internal files are not re-exported. Example:
  ```ts
  // src/symbolon/index.ts
  export { authMiddleware } from "./middleware.js";
  export { hashPassword, verifyPassword } from "./passwords.js";
  export { createSession, validateSession } from "./sessions.js";
  export { createToken, verifyToken } from "./tokens.js";
  export { rbacMiddleware, requireRole } from "./rbac.js";
  ```

- `src/*/meta.ts` — Create module identity constants for each module:
  ```ts
  // src/symbolon/meta.ts
  export const MODULE = "symbolon" as const;
  export const LOG_PREFIX = "symbolon" as const;
  export const ERROR_MODULE = "symbolon" as const;
  export const ROUTE_PREFIX = "/api/auth" as const;
  ```

- Migrate all cross-module imports to use path aliases and barrel imports. Internal imports within a module (e.g., `symbolon/middleware.ts` importing `./passwords.js`) stay as relative paths.

- Replace all hardcoded logger name strings with `meta.LOG_PREFIX`, error module strings with `meta.ERROR_MODULE`, route prefix strings with `meta.ROUTE_PREFIX`.

- Create event name registries where event patterns exist (e.g., `src/melete/events.ts` for `distill:*` events, `src/semeion/events.ts` for signal events).

**Acceptance Criteria:**
- [ ] All gnomon modules have path aliases in tsconfig
- [ ] All gnomon modules have barrel `index.ts` with explicit public surface
- [ ] All gnomon modules have `meta.ts` with identity constants
- [ ] All cross-module imports use `@module` aliases (no `../module/file.js`)
- [ ] All logger names reference `meta.LOG_PREFIX`, not string literals
- [ ] All error module identifiers reference `meta.ERROR_MODULE`
- [ ] Event registries exist for modules that emit events
- [ ] All tests pass, typecheck clean
- [ ] A simulated rename (temporarily aliasing one module) requires only tsconfig + meta.ts changes

**Testing:**
- Existing test suites pass without modification (import resolution via tsconfig)
- Add a meta.ts validation test that ensures every module directory has a `meta.ts` and `index.ts`
- Add an import lint rule or test that flags direct cross-module imports (bypassing barrel)

---

### Phase 2: Workspace File Renames — GOALS.md → TELOS.md, MEMORY.md → MNEME.md

**Scope:** Rename the two workspace files that name implementations rather than modes of attention. Update all code that references them by filename.

**Changes:**

- `nous/_example/GOALS.md` → `nous/_example/TELOS.md`
- `nous/_example/MEMORY.md` → `nous/_example/MNEME.md`

- `src/taxis/scaffold.ts` — Update scaffold template references:
  - `"GOALS.md"` → `"TELOS.md"` in the file list and generation prompt
  - `"MEMORY.md"` → `"MNEME.md"` in the file list and generation prompt
  - Update the scaffold instructions that reference these files by name

- `src/taxis/scaffold.test.ts` — Update test expectations

- `src/nous/bootstrap.ts` — Update file list entries:
  - `{ name: "GOALS.md", ... }` → `{ name: "TELOS.md", ... }`
  - `{ name: "MEMORY.md", ... }` → `{ name: "MNEME.md", ... }`

- `src/nous/bootstrap.test.ts` — Update test references

- `src/nous/bootstrap-diff.ts` — Update `SEMI_STATIC_FILES` set

- `src/organon/built-in/note.ts` — Update help text reference to MEMORY.md

- `src/portability/export.test.ts` and `src/portability/import.test.ts` — Update test fixtures

- **Deployed agent workspaces** (`nous/syn/`, `nous/demiurge/`, `nous/syl/`, `nous/akron/`) — Rename actual files. This is a runtime change that must happen in the same deployment as the code change.

- **Agent AGENTS.md templates** — Update any documentation references to the old filenames.

**Backward compatibility:** The scaffold and bootstrap should accept both old and new names during a transition period (check for `TELOS.md`, fall back to `GOALS.md`). Remove fallback after all deployed agents are migrated.

**Acceptance Criteria:**
- [ ] `_example/` template uses TELOS.md and MNEME.md
- [ ] Scaffold generates new agents with TELOS.md and MNEME.md
- [ ] Bootstrap loads TELOS.md and MNEME.md (with fallback to old names)
- [ ] All deployed agent workspaces have files renamed
- [ ] Export/import handles both old and new filenames
- [ ] No references to "GOALS.md" or "MEMORY.md" remain in runtime code (except fallback)
- [ ] Tests pass, typecheck clean

**Testing:**
- Scaffold test generates files with new names
- Bootstrap test loads files with new names
- Bootstrap test loads files with old names (fallback)
- Export/import roundtrip works with both name variants

---

### Phase 3: Module Rename — portability → autarkeia

**Scope:** Rename the last non-gnomon runtime module. With Phase 1 infrastructure in place, this should be significantly cheaper than the symbolon/melete rename.

**Why autarkeia:** αὐτο (self) + ἀρκέω (to suffice) — self-sufficiency. The export works because the agent is complete-in-itself. Autarkeia is not a property the module adds; it's the precondition the module tests and enacts. See issue #227 for the full L1-L4 layer test.

**Changes:**

- `src/portability/` → `src/autarkeia/`
- `tsconfig.json` — Add `@autarkeia/*` path alias
- `src/autarkeia/meta.ts` — Create identity constants
- `src/autarkeia/index.ts` — Create barrel export
- Update imports in `src/aletheia.ts` and `src/entry.ts` (only 4 references, and with Phase 1 aliases these will be `@autarkeia` imports)
- `AgentFile` type name stays — it names the artifact, not the module

**Acceptance Criteria:**
- [ ] Module directory renamed
- [ ] Path alias added
- [ ] Barrel export and meta.ts created
- [ ] All imports updated to `@autarkeia`
- [ ] Tests pass, typecheck clean
- [ ] `AgentFile` export/import functionality unchanged

**Testing:**
- Existing export/import tests pass
- Verify barrel export surfaces all public types

---

### Phase 4: Sub-Agent Role Renames

**Scope:** Rename the five sub-agent roles from job functions to modes of attention. This touches role definitions, type unions, dispatch code, and documentation.

**Renames:**

| Current | New | Greek | Mode of attention |
|---------|-----|-------|-------------------|
| `coder` | `tekton` | τέκτων (craftsman) | Receives a plan and builds it |
| `explorer` | `theoros` | θεωρός (observer) | Sees without acting — official observer |
| `researcher` | `zetetes` | ζητητής (seeker) | Pursues until found, synthesizes |
| `reviewer` | `kritikos` | κριτικός (judge) | Separates, evaluates, judges |
| `runner` | `ergates` | ἐργάτης (worker) | Executes, reports, does not interpret |

See issue #229 for full topology analysis.

**Changes:**

- `src/nous/roles/prompts/` — Rename files: `coder.ts` → `tekton.ts`, etc.
- `src/nous/roles/prompts/*.ts` — Update exported constant names (`CODER_PROMPT` → `TEKTON_PROMPT`, etc.) and system prompt text
- `src/nous/roles/index.ts` — Update imports, `RoleName` type union, `ROLES` record keys
- `src/organon/config/sub-agent-roles.ts` — Update `ROLE_NAMES` array
- `src/organon/config/sub-agent-roles.test.ts` — Update test references
- `src/mneme/store.ts` — Update `role` type union on the session/turn type
- `src/nous/pipeline/types.ts` — Update `role` type union
- `src/organon/built-in/sessions-dispatch.test.ts` — Update test fixtures
- `src/organon/built-in/plan-propose.ts` — Update role name references and enum
- `src/dianoia/researcher.ts` — Update role string literals
- `src/dianoia/roadmap.ts` — Update role string literals
- `src/dianoia/verifier.ts` — Update role string literals
- `src/dianoia/context-packet.test.ts` — Update test fixtures
- `nous/_example/AGENTS.md` — Update delegation table in template
- All deployed agent `AGENTS.md` files — Update delegation documentation

**Backward compatibility:** The dispatch system should accept both old and new role names during a transition period. `isValidRole()` recognizes both. A mapping from old → new allows existing agent prompts (AGENTS.md) that reference "coder" to still resolve. Remove after all agent workspaces are updated.

**Acceptance Criteria:**
- [ ] All 5 role files renamed with updated prompts
- [ ] `RoleName` type uses new names
- [ ] `ROLES` record uses new names
- [ ] All dispatch code uses new names
- [ ] Backward-compatible mapping from old names exists
- [ ] Agent documentation updated
- [ ] Tests pass, typecheck clean

**Testing:**
- Existing dispatch tests pass with new names
- Add test that old role names resolve via compatibility mapping
- Role config test validates all 5 roles have prompts, tools, and limits

---

### Phase 5: Cross-Cutting Constant Consolidation

**Scope:** Audit the codebase for repeated magic strings and numbers outside of module identity. Extract to domain-scoped constant files. This is the "everything else" phase that Phase 1 doesn't cover.

**Target areas:**

- **Event names** — Any `emit("string:literal")` or `on("string:literal")` patterns not already covered by module event registries
- **Header names** — Custom HTTP headers, auth header constants
- **Timeout values** — Retry intervals, connection timeouts, cooldown periods scattered across files
- **Capability keys** — Tool names, permission strings referenced in multiple places
- **Config keys** — Configuration property names used in multiple files
- **Error codes** — Numeric or string error identifiers

**Changes:**

- `src/koina/constants.ts` (or per-domain files) — Extract shared constants
- Update all consumers to reference constants instead of literals
- Add an ESLint rule or CI check to flag new magic strings in cross-module positions

**Acceptance Criteria:**
- [ ] Audit complete — all repeated cross-module magic strings identified
- [ ] Constants extracted to appropriate scope (module-level or shared)
- [ ] Consumers updated to reference constants
- [ ] CI lint rule prevents regression
- [ ] Tests pass, typecheck clean

**Testing:**
- Constants file has comprehensive exports
- Lint rule catches new magic strings (at least a snapshot test of the pattern)

---

## Dependency Graph

```
Phase 1 (boundary infrastructure)
  ├── Phase 2 (workspace file renames) — uses barrel/meta patterns from Phase 1
  ├── Phase 3 (autarkeia rename) — trivial with Phase 1 aliases
  └── Phase 4 (role renames) — uses meta.ts pattern, lighter scope
Phase 5 (constant consolidation) — after Phases 1-4, broader audit
```

Phases 2, 3, and 4 are independent of each other once Phase 1 lands. They can be parallelized or reordered.

---

## Open Questions

1. **Path alias resolution at runtime.** tsconfig `paths` are a compile-time feature. The runtime needs a module resolver that respects them. Options: `tsc-alias` post-compile transform, `tsx` with `--tsconfig-paths`, or Node.js `--import` with a custom loader. Need to verify which approach works with the current build pipeline.

2. **Barrel export granularity.** Should barrel exports re-export everything public, or be selective? Selective is more maintainable (explicit surface) but requires updating the barrel when adding new public APIs. Recommendation: selective — the barrel IS the module contract.

3. **ESLint rule for import discipline.** `eslint-plugin-import` has a `no-internal-modules` rule that can enforce barrel-only imports. Need to verify compatibility with the current ESLint config and TypeScript path aliases.

4. **Daemon module.** `src/daemon/` hasn't been discussed for renaming. It may already be descriptive enough (daemons are a well-understood concept), or it may warrant a gnomon name. Defer until the naming reveals itself.

---

## References

- `docs/gnomon.md` — The naming system and philosophy
- PR #225 — The 49-file rename that exposed the O(n) cost
- Issue #224 — auth → symbolon, distillation → melete (completed)
- Issue #226 — Names and constants as variables
- Issue #227 — portability → autarkeia
- Issue #228 — GOALS.md → TELOS.md, MEMORY.md → MNEME.md
- Issue #229 — Sub-agent role renames
- `ARCHITECTURE.md` — Current module dependency matrix
