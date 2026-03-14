# Git and Workflow Standards

> Additive to README.md. Read that first. Everything here covers commit format, branching, worktrees, and PR discipline.

---

## Conventional Commits

All commits use conventional commit format: `type(scope): description`

| Type | When |
|------|------|
| `feat` | New capability |
| `fix` | Bug fix |
| `refactor` | Code change that neither fixes nor adds |
| `test` | Adding or fixing tests |
| `chore` | Build, CI, docs, tooling |
| `perf` | Performance improvement |
| `ci` | CI/CD changes |

- Present tense, imperative mood: "add X" not "added X"
- First line ≤ 72 characters
- Body wraps at 80 characters
- One logical change per commit
- Scope is the module/crate/component name: `feat(mneme): add graph score aggregation`

## Branching

| Type | Pattern | Example |
|------|---------|---------|
| Feature | `feat/<description>` | `feat/audiobook-chapters` |
| Bug fix | `fix/<description>` | `fix/gapless-gap` |
| Chore | `chore/<description>` | `chore/update-deps` |

- Always branch from `main`
- Always rebase before pushing (linear history)
- Never commit directly to `main`
- Squash merge is the default for PRs

## Worktrees for Parallel Work

When multiple agents or sessions work in parallel, use git worktrees for full filesystem isolation:

```bash
git worktree add ../repo-feat-name -b feat/name main
cd ../repo-feat-name
# work, commit, push, PR
# after merge:
git worktree remove ../repo-feat-name
git branch -d feat/name
```

One task, one worktree. Don't reuse worktrees. Build and test in the worktree, not in main.

## PR Discipline

- PR title matches the conventional commit format
- PR description states what changed and why — not how (the code shows how)
- Every PR targets `main`
- Lint and type checks pass before pushing (don't rely solely on CI)

## CI Validation Gate

Every merge requires four passing checks: lint, type-check, test, and dependency audit. No exceptions, no manual overrides. Each language file specifies the exact commands under "Build/validate."

## Authorship

All commits use the operator's identity. Agents are tooling, not contributors.
