# Contributing to Aletheia

## Setup

```bash
git clone https://github.com/forkwright/aletheia.git
cd infrastructure/runtime
npm install
git config core.hooksPath .githooks
```

## Development

```bash
npx tsdown                  # Build
npm run typecheck           # tsc --noEmit
npm run lint:check          # oxlint
npm run precommit           # Typecheck + lint (pre-commit gate)
```

### Local Validation

Run **only** typecheck and lint during development:

```bash
npm run typecheck && npm run lint:check
```

For targeted testing of specific functionality:

```bash
npx vitest run src/path/to/specific.test.ts
```

**Never run `npm test` or the full suite locally.** CI handles full test runs. Local
full-suite runs are slow (84+ seconds), frequently time out agent sessions, and
duplicate what CI already does.

### Pre-commit Hook

The `.githooks/pre-commit` hook runs typecheck + lint automatically. It does not
run tests — that's CI's job.

## Git

### Authorship

All commits use Cody's author identity:

```
git config user.name "Cody Kickertz"
git config user.email "cody.kickertz@gmail.com"
```

Agents are tooling, not contributors. The git log reads like one person built
this — because one person directed it.

### Branch Convention

| Type | Pattern | Example |
|------|---------|---------|
| Spec work | `spec<NN>/<description>` | `spec14/dev-workflow` |
| Bug fix | `fix/<description>` | `fix/distillation-overflow` |
| Feature (non-spec) | `feat/<description>` | `feat/gcal-rebuild` |
| Chore/docs | `chore/<description>` | `chore/readme-update` |

Rules:
- Always branch from `main`
- Always `git pull --rebase origin main` before pushing
- Never commit directly to `main` (except docs-only or trivial config changes)

### Commit Messages

Format:

```
<type>: <concise description>

[optional body — what and why, not how]

[optional footer — Spec: NN, Closes #NN, Breaking: description]
```

**Types:** `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `ci`, `perf`

Rules:
- Present tense, imperative mood: "add X" not "added X"
- First line ≤72 characters
- Body wraps at 80 characters
- Reference spec number for spec work: `Spec: 14`
- One logical change per commit

Examples:

```
feat: add message queue for mid-turn course correction

Human messages sent during an active turn are queued and injected after
the current tool call completes. Queue lives on SessionStore, checked
between tool executions in both streaming and buffered paths.

Spec: 04
```

```
fix: remove future annotations from memory sidecar routes

Caused intermittent TypeError on graph_enhanced_search. Python 3.12
supports modern type syntax natively — the import was unnecessary.
```

### Squash Policy

**Always squash merge.** No merge commits, no rebase-merge.

Every PR becomes a single commit on `main`. The squash commit message = PR title
\+ summary from the PR description. The branch preserves detailed work history
for anyone who cares.

## Pull Requests

### Workflow

```
1. Create branch from main
2. Make changes, commit, push
3. Create PR with structured description
4. CI must pass — no merging red PRs
5. Review (Syn reviews agent work, Cody reviews when needed)
6. Squash merge with clean commit message
7. Branch auto-deleted after merge
```

### PR Description

Use the template (`.github/pull_request_template.md`). Every PR needs:

- **What:** One-paragraph summary
- **Why:** Problem solved or spec phase implemented
- **Changes:** Bullet list of significant changes
- **Spec:** Reference (`Spec: NN Phase N`) if applicable
- **Testing:** How it was tested

### Branch Cleanup

- Branches deleted automatically after merge (GitHub setting)
- Local branches pruned with `git fetch --prune`
- Stale branches (>7 days with no PR) flagged during weekly maintenance

## Agent Task Dispatch

When dispatching work to Claude Code or sub-agents, every task uses this template:

```markdown
# Task: <title>

## Branch
`<branch-name>` (create from main)

## Scope
<what to do — specific files, functions, behaviors>

## Constraints
- Git author: Cody Kickertz <cody.kickertz@gmail.com>
- ONE squashed commit before pushing
- Commit message: `<type>: <description>\n\nSpec: <NN>`
- Push the branch. Do NOT create a PR.
- Do NOT modify files outside scope.
- Do NOT add dependencies without noting in commit body.
- Run `npm run typecheck && npm run lint:check` before pushing.
  Fix any errors your changes introduce.
- Do NOT run the full test suite.

## Acceptance Criteria
- [ ] <specific, testable conditions>

## Context
<relevant background — link to specs/files, keep minimal>
```

The dispatching agent (typically Syn) creates the PR after reviewing the pushed
branch. This separates execution from review.

## Code Standards

Full reference: [DEVELOPMENT.md](docs/DEVELOPMENT.md#code-style-conventions). Summary:

**Self-documenting code.** One-line file headers. Inline comments only for
*why*, never *what*.

**Typed errors.** All errors extend `AletheiaError`. Error codes in
`koina/error-codes.ts`. Never throw strings or bare `Error`. Non-critical ops
use `trySafe`/`trySafeAsync` from `koina/safe.ts`.

**No silent catch.** Every catch block must either log, rethrow, return a meaningful value, or include an inline `/* reason */` comment explaining why the error is intentionally discarded.

**Naming:** Files `kebab-case`, classes `PascalCase`, functions `camelCase`
verb-first, constants `UPPER_SNAKE`, events `noun:verb`. Persistent names for
modules, subsystems, and agents follow the [Gnomon naming system](docs/gnomon.md) —
Greek names that unconceal essential natures, not describe implementations. Run the
layer test (L1-L4) before naming anything that will persist.

**TypeScript:** Strict mode, `.js` import extensions, bracket notation for index
access.

**Testing:** Behavior not implementation, one assertion per test, descriptive
names, same-directory `*.test.ts` files.

## Dianoia Module

Dianoia is the persistent multi-phase planning module at `infrastructure/runtime/src/dianoia/`. It adds planning project state to SQLite, drives a state machine (`DianoiaOrchestrator`), and coordinates wave-based parallel execution of planning subagents. See `docs/specs/archive/DECISIONS.md` (Dianoia section) for the full design.

### Key Patterns

- **Injected-db**: all orchestrators take `Database.Database` in their constructor
- **Constructor-injected dispatchTool**: not imported as a global; passed in at construction time
- **OrThrow pattern**: required lookups throw a typed `AletheiaError` on miss (e.g., `getProjectOrThrow`)

### Gotchas

**Gotcha 1 — Migration propagation:**
Every `makeDb()` helper in `src/dianoia/*.test.ts` must include ALL migrations through the current version. When a new migration is added, update ALL test helpers: `store.test.ts`, `orchestrator.test.ts`, `researcher.test.ts`, `requirements.test.ts`, `roadmap.test.ts`, `roadmap-tool.test.ts`, `execution.test.ts`, `verifier.test.ts`, `checkpoint.test.ts`, and `dianoia.integration.test.ts`.

**Gotcha 2 — exactOptionalPropertyTypes:**
`tsconfig.json` enables `exactOptionalPropertyTypes`. When merging objects with optional fields, use conditional spread:
```typescript
// Wrong:
const merged = { ...base, optionalField: value ?? undefined };
// Right:
const merged = { ...base, ...(value !== undefined ? { optionalField: value } : {}) };
```

**Gotcha 3 — oxlint require-await:**
`ToolHandler.execute()` implementations that are synchronous in some branches must use `return Promise.resolve(result)` instead of `async` keyword. The `async` keyword on a function with no `await` triggers `eslint(require-await)`.

**Gotcha 4 — Orchestrator registration:**
New orchestrators follow the `NousManager` setter/getter pattern. They are set in `createRuntime()`, retrieved in `server.ts` via `manager.get*()`, and spread into `RouteDeps` using conditional spread (required by `exactOptionalPropertyTypes`):
```typescript
// server.ts pattern:
const orchValue = manager.getMyOrchestrator();
const deps: RouteDeps = {
  ...base,
  ...(orchValue !== undefined ? { myOrchestrator: orchValue } : {}),
};
```

For full design detail, see `docs/specs/archive/DECISIONS.md` (Dianoia — Persistent Multi-Phase Planning Runtime section).

## Reporting Issues

- **Bugs:** [bug report template](.github/ISSUE_TEMPLATE/bug_report.md)
- **Features:** [feature request template](.github/ISSUE_TEMPLATE/feature_request.md)
- **Security:** [SECURITY.md](.github/SECURITY.md) — do not open public issues

## License

By contributing, you agree to [AGPL-3.0](LICENSE).
