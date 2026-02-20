## Development Workflow

### Local Validation

Run **only** typecheck and lint during development:

```bash
npm run typecheck && npm run lint:check
```

For targeted testing of specific functionality:

```bash
npx vitest run src/path/to/specific.test.ts
```

**Never run `npm test` or the full suite locally.** CI handles full test runs.

### Git Rules

- **Author:** `Cody Kickertz <cody.kickertz@gmail.com>` (always)
- **Branch from main:** `git checkout -b <type>/<description> main`
- **Rebase before push:** `git pull --rebase origin main`
- **Commit format:** `<type>: <description>` — types: feat, fix, refactor, docs, test, chore, ci, perf
- **One logical change per commit.** Squash micro-commits before pushing.
- **Always push after commit.** Commits without push don't exist.

### Branch Naming

| Type | Pattern | Example |
|------|---------|---------|
| Spec work | `spec<NN>/<description>` | `spec14/dev-workflow` |
| Bug fix | `fix/<description>` | `fix/distillation-overflow` |
| Feature | `feat/<description>` | `feat/gcal-rebuild` |
| Chore/docs | `chore/<description>` | `chore/readme-update` |

### Delegation

When delegating to sub-agents or Claude Code, include in every task:
- Branch name
- Scope (specific files and changes)
- Single squashed commit with proper message format
- "Push branch, do NOT create PR"
- "Run typecheck + lint before pushing"

The orchestrator creates the PR after reviewing the branch.

Full task dispatch template: [CONTRIBUTING.md](/CONTRIBUTING.md#agent-task-dispatch)
