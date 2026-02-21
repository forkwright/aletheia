# Git PR Merge and Sync Workflow
Merge a pull request using squash commits and synchronize the local repository with the remote main branch.

## When to Use
When you need to merge a pull request into the main branch with a squash commit strategy and ensure your local repository is up-to-date with the latest remote changes. Useful for cleanup workflows after PR merging.

## Steps
1. Checkout the main branch locally
2. Attempt to merge the pull request using squash strategy with a custom commit message
3. Handle cases where the PR is already merged (graceful error handling)
4. Pull the latest changes from the remote main branch
5. Verify the merge completed by listing remaining open PRs

## Tools Used
- exec: Execute shell commands for git operations (checkout, pull) and GitHub CLI operations (pr merge, pr list)