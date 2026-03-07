## Development Workflow

### Local Validation

Run targeted tests during development:

```bash
cargo test -p <affected-crate>
cargo clippy --workspace --all-targets -- -D warnings
```

Full suite as a final gate before PR:

```bash
cargo test --workspace
```

### Git Rules

- **Author:** `forkwright <forkwright@users.noreply.github.com>` (always)
- **Branch from main:** `git checkout -b <type>/<description> main`
- **Rebase before push:** `git pull --rebase origin main`
- **Commit format:** `<type>(<scope>): <description>` - types: feat, fix, refactor, docs, test, chore, ci, perf
- **One logical change per commit.** Squash micro-commits before pushing.
- **Always push after commit.** Commits without push don't exist.

### Branch Naming

| Type | Pattern | Example |
|------|---------|---------|
| Feature | `feat/<description>` | `feat/recall-pipeline` |
| Bug fix | `fix/<description>` | `fix/session-timeout` |
| Docs | `docs/<description>` | `docs/deployment-guide` |
| Refactor | `refactor/<description>` | `refactor/config-cascade` |
| Chore | `chore/<description>` | `chore/update-deps` |

### Delegation

When delegating to sub-agents or Claude Code, include in every task:
- Branch name
- Scope (specific files and changes)
- Single squashed commit with proper message format
- "Push branch, do NOT create PR"
- "Run clippy + tests before pushing"

The orchestrator creates the PR after reviewing the branch.

Workflow and standards: [CLAUDE.md](../../../CLAUDE.md)
