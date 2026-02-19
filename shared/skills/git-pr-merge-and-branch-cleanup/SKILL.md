# Git PR Merge and Branch Cleanup
Merge a pull request into main branch and clean up the associated feature branch locally and remotely.

## When to Use
After a pull request has been reviewed and approved, use this pattern to merge it into the main branch, sync local changes, and remove the feature branch to maintain repository cleanliness.

## Steps
1. Attempt to merge the pull request using `gh pr merge` with squash option and a descriptive commit message
2. If merge is already complete, pull the latest changes from origin/main to sync local branch
3. Verify the merge by viewing recent commit history
4. Delete the feature branch locally using `git branch -d`
5. Delete the feature branch from remote using `git push origin --delete`
6. Confirm cleanup completion

## Tools Used
- exec: Execute git commands for branch operations, pulling changes, and viewing history; execute gh CLI for PR merging
