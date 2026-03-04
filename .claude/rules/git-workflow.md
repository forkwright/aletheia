# Git Workflow Rules

Rules for branching, parallel development, and pull request discipline.

---

## Git Worktrees for All Feature Work

Never develop on `main` directly. Every task gets its own git worktree.

**Why:** Multiple agents (or Claude Code sessions) may execute in parallel against the same repo. Worktrees provide full filesystem isolation - each has its own working tree, index, and HEAD. No conflicts, no stash juggling, no accidental cross-contamination.

### Setup

```bash
# From the repo root (main branch, always clean)
git worktree add ../aletheia-feat-<name> -bfeat/<name> main

# Work happens in the worktree directory
cd ../aletheia-feat-<name>

# When done - after PR is merged
cd /path/to/main/repo
git worktree remove ../aletheia-feat-<name>
git branch -d feat/<name>
```

### Rules

1. **Main is read-only.** The main worktree stays on `main`, always clean. It's the source for creating new worktrees, not a place to develop.

2. **One task, one worktree.** Don't reuse a worktree for a different task. Create fresh, work, PR, remove.

3. **Branch naming:** `feat/<name>`, `fix/<name>`, or `chore/<name>`. Match the conventional commit prefix.

4. **Build in the worktree.** Run `cargo check`, `cargo clippy`, `cargo test` inside the worktree - not in main. The worktree has its own `target/` directory (or shares via `CARGO_TARGET_DIR` if configured).

5. **Clean up after merge.** Remove the worktree and delete the local branch once the PR lands on main.

6. **Rebase, don't merge.** If main has advanced while you're working, rebase your feature branch onto main. Keep history linear.

### Parallel Work

Multiple worktrees can exist simultaneously. Independent tasks won't conflict because each has its own filesystem. Tasks that touch overlapping files should be sequenced - whichever merges second rebases onto the updated main.

Compliant:
```bash
# Two parallel tasks, each in its own worktree
git worktree add ../aletheia-feat-recall -bfeat/recall main
git worktree add ../aletheia-fix-rrf -bfix/rrf-encoding main
# Each works independently, PRs reviewed and merged separately
```

Non-compliant:
```bash
# Working directly on main
git checkout main
# editing files...
git commit -m "feat: add recall pipeline"

# Stashing to switch tasks
git stash
git checkout -bfeat/other-thing
# This creates race conditions with parallel agents
```

---

## Commit Discipline

### Conventional Commits

All commits use conventional commit format: `type(scope): description`

| Type | When |
|------|------|
| `feat` | New capability |
| `fix` | Bug fix |
| `refactor` | Code change that neither fixes nor adds |
| `chore` | Build, CI, docs, tooling |
| `test` | Adding or fixing tests |

Scope is the crate or module name: `feat(nous): add history stage`, `fix(mneme): graph score aggregation`.

### PR Creation

```bash
# From the worktree
git push -u origin feat/<name>
gh pr create --title "feat(scope): description" --body "..."
```

Every PR targets `main`. Squash merge is the default. The PR description should state what changed and why - not how (the code shows how).

