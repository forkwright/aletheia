# Batch PR Merge and Branch Cleanup
Merge multiple pull requests in sequence and clean up their associated branches from local repository.

## When to Use
When you need to merge a series of reviewed and approved pull requests into the main branch, update your local repository state, and remove the now-obsolete feature branches to keep the repository clean.

## Steps
1. List open pull requests to identify which ones to merge (optional verification step)
2. Merge each pull request sequentially using `gh pr merge` with squash option and descriptive commit messages
3. Checkout main branch and pull latest changes from remote
4. Fetch and prune remote branches to sync local tracking state
5. Verify current branch state with `git branch -v`
6. Delete merged feature branches locally using `git branch -D` with multiple branch names
7. Verify cleanup completion by listing remaining open PRs and local branches

## Tools Used
- gh pr list: identify and verify open pull requests before merging
- gh pr merge: merge pull requests with squash strategy and custom commit messages
- git checkout: switch to main branch
- git pull: update local main with remote changes
- git fetch --prune: sync remote branch tracking and remove stale references
- git branch: view and verify local branches
- git branch -D: delete multiple local branches after merge