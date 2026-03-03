# Agent Contribution Policy

## Rule 1: All automated agent commits go through PRs

No agent — including the GSD agent — pushes directly to main. Every automated commit opens a PR and must pass CI before merging.

**Exceptions:** None. Branch protection enforces this.

## Rule 2: Commit scope limits

| Agent Role | Max files per commit | Max LOC per commit |
|-----------|---------------------|-------------------|
| Coder (sub-agent) | 10 | 500 |
| Reviewer | 0 (review only) | 0 |
| Orchestrator (Syn) | 20 | 1000 |
| Dependabot | Unlimited (deps only) | Unlimited |

Commits exceeding scope limits are split into multiple PRs.

## Rule 3: Quality gates

All agent PRs must pass:
- [ ] TypeScript type check (`npm run typecheck`)
- [ ] Lint (`npm run lint:check`)
- [ ] Commit message format (`commitlint`)
- [ ] No secrets in diff (`trufflehog`)

## Rule 4: Review requirements

| Change Type | Review Required |
|-------------|----------------|
| Runtime source | Human approval |
| UI source | Human approval |
| Documentation | Auto-merge after CI |
| Dependency updates | Auto-merge if patch, human if minor/major |
| Config/tooling | Human approval |

## Rule 5: Attribution

All commits use the operator's identity (`forkwright <forkwright@users.noreply.github.com>`). No `Co-authored-by: Claude` or agent attribution in public history. Agents are tools, not authors.

## Rule 6: Branching

- **Branch naming:** `<type>/<description>` (e.g., `feat/memory-pipeline`, `fix/auth-token`)
- **One concern per branch** — no mixing features
- **Always squash merge** — no merge commits
- **Delete branch after merge**
