# Git Repository Cleanup and Verification
Identify and remove deleted remote branches from local Git repository after fetching latest remote state.

## When to Use
When you need to clean up local Git branches that have been deleted from the remote repository, particularly after a git fetch operation. Useful for maintaining a clean local branch list and preventing confusion from stale local branches.

## Steps
1. Fetch all remote branches and prune deleted ones using `git fetch --all --prune`
2. List all remote branches to identify which branches exist on remote using `git branch -r`
3. List all local branches to see current local state using `git branch`
4. Delete specific local branches that no longer exist remotely using `git branch -D <branch-name>`
5. Verify the repository state by checking recent commits using `git log --oneline -n <count>`

## Tools Used
- exec: Execute shell commands to run Git operations and verify repository state