# Git Pull Request Merge and Branch Cleanup
Merge a pull request and clean up local and remote feature branches after successful integration.

## When to Use
After a pull request has been approved and is ready to merge, use this pattern to merge it into the main branch, sync local changes, and clean up associated feature branches to maintain repository hygiene.

## Steps
1. Checkout main branch and restore any stashed changes with `git checkout main && git stash pop`
2. Merge the pull request using `gh pr merge <pr_number> --squash` with an appropriate commit message
3. Pull the latest changes from remote with `git pull` and verify the merge with `git log`
4. Delete the remote feature branch with `git push origin --delete <branch_name>`
5. Prune stale remote references with `git remote prune origin`
6. Delete the local feature branch with `git branch -D <branch_name>`

## Tools Used
- exec: Execute git commands for branch management and GitHub CLI commands for PR operations