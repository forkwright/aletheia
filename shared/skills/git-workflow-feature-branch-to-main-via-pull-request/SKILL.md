# Git Workflow: Feature Branch to Main via Pull Request
Execute a complete feature development workflow from staging changes through merging to main branch.

## When to Use
When you need to commit local changes, create a pull request, merge it to the main branch, and clean up the feature branch in a single coordinated sequence.

## Steps
1. Stage all changes and verify the diff summary using `git add -A && git diff --cached --stat`
2. Commit staged changes with a descriptive message including feature tags and issue references
3. Create a pull request using the GitHub CLI with matching title and detailed body description
4. Merge the pull request using squash merge strategy to maintain clean history
5. Switch to main branch, pull latest changes with rebase, and delete the local feature branch

## Tools Used
- exec: Execute shell commands for git operations and GitHub CLI interactions