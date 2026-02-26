# Code Review and PR Validation Workflow
Systematically review, validate, and merge a pull request with comprehensive testing before integration.

## When to Use
When you need to evaluate a pull request for quality assurance before merging, including checking code changes, running linters, type checking, and ensuring all tests pass.

## Steps
1. List open PRs and branches to identify the target PR
2. Fetch PR metadata (title, body, statistics, file changes, mergeable status)
3. Get the list of changed files to understand scope
4. Fetch the feature branch and view commit history relative to main
5. Checkout the feature branch locally
6. Run TypeScript compiler (tsc) to check for type errors
7. Run linter (oxlint) on relevant source directories
8. Run project-specific lint checks (npm run lint:check)
9. Switch back to main branch
10. Merge the PR with squash strategy and delete remote branch
11. Pull latest changes and clean up local stale branches
12. Prune remote branches and finalize repository state

## Tools Used
- exec: Execute shell commands for git operations, GitHub CLI queries, and linting/type-checking tools
- gh pr list: Identify PRs and their status
- gh pr view: Retrieve detailed PR metadata
- gh pr diff: List changed files in PR
- git checkout/fetch/log: Branch management and history inspection
- npx tsc: TypeScript type checking
- npx oxlint: Linting validation
- npm run lint: Project-specific linting
- gh pr merge: Merge PR with options
- git pull/branch/remote: Repository cleanup and synchronization
