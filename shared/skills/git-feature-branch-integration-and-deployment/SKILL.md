# Git Feature Branch Integration and Deployment

Integrate a feature branch into main, verify builds succeed, and clean up remote tracking.

## When to Use
When you need to merge a feature branch that was previously worked on, ensure it's properly integrated into main, verify the build passes, and clean up the feature branch from both local and remote repositories.

## Steps
1. Check current git status and recent commit history to understand the state of the repository
2. Reset local main branch to match origin/main to ensure clean state
3. Inspect the feature branch and compare its changes against main using git log and diff
4. Search reflog to locate the specific commit containing the feature work
5. Inspect the target commit to verify it contains the desired changes
6. Switch to main branch and cherry-pick the feature commit
7. Run TypeScript type-checking to validate code integrity
8. Build all relevant project modules (runtime and UI) to verify compilation succeeds
9. Push the updated main branch to remote and delete the feature branch locally and remotely

## Tools Used
- exec: Used for all git operations (status, log, reset, cherry-pick, push, branch deletion), build commands (tsc, npm run build), and verification steps