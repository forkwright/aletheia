# Git Workflow: Commit, Close Related PRs, and Verify Integration

Commit code changes, close multiple related pull requests with a summary message, and verify the integration by checking remaining open PRs and recent commit history.

## When to Use
After completing a feature or fix that consolidates multiple pull requests, you need to:
- Commit the changes to the main branch
- Close multiple related PRs with a consistent closure message
- Verify the integration was successful by reviewing open PRs and recent commits

## Steps
1. Navigate to the repository root directory
2. Stage and commit the changes with a descriptive message that references what was fixed/integrated
3. Close multiple related pull requests in a loop, providing a consistent summary message explaining the merge and any related commits
4. Verify successful integration by listing remaining open PRs and viewing recent commit history (last 8 commits)

## Tools Used
- exec: Used to run git commands (add, commit), GitHub CLI commands (gh pr close, gh pr list), and git log for verification