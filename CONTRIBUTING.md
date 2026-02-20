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

**Never empty catch.** Every catch logs, rethrows, or returns a meaningful value.

**Naming:** Files `kebab-case`, classes `PascalCase`, functions `camelCase`
verb-first, constants `UPPER_SNAKE`, events `noun:verb`.

**TypeScript:** Strict mode, `.js` import extensions, bracket notation for index
access.

**Testing:** Behavior not implementation, one assertion per test, descriptive
names, same-directory `*.test.ts` files.

## Reporting Issues

- **Bugs:** [bug report template](.github/ISSUE_TEMPLATE/bug_report.md)
- **Features:** [feature request template](.github/ISSUE_TEMPLATE/feature_request.md)
- **Security:** [SECURITY.md](.github/SECURITY.md) — do not open public issues

## License

By contributing, you agree to [AGPL-3.0](LICENSE).
