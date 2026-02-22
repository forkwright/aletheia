# Git PR Merge & Branch Cleanup Workflow
Systematically merge multiple pull requests, rebase dependent branches, rebuild projects, and clean up merged branches.

## When to Use
When you need to integrate multiple related pull requests into main, handle dependent feature branches that need rebasing, verify builds succeed, and clean up stale branches after merging.

## Steps
1. Review and inspect PR diffs using `git diff` to understand changes across multiple branches
2. Merge pull requests sequentially using `gh pr merge` with squash strategy and auto-delete flag
3. After merging PRs that main depends on, checkout dependent feature branches and rebase them against updated main
4. Force-push rebased branches to remote to update PR state
5. Pull latest main to verify all merges integrated cleanly
6. Run project builds (npm build, tsdown) to verify no breaking changes
7. Fetch and prune remote branches to identify stale local branches
8. Delete merged feature branches locally using `git branch -D`

## Tools Used
- exec: Execute shell commands for git operations, gh CLI commands, and build processes
- git diff: Inspect changes in branches before merging
- gh pr merge: Merge PRs with squash strategy and cleanup options
- git rebase: Update dependent branches against updated main
- git branch: List and delete local branches
- npm/build tools: Verify project builds succeed after integrations
