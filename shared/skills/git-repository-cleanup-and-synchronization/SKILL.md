# Git Repository Cleanup and Synchronization
Clean up local git branches and synchronize with remote repository state.

## When to Use
When you need to clean up a git repository by removing merged local branches and pruning deleted remote tracking branches to keep the repository state synchronized with the remote.

## Steps
1. Checkout the main branch to ensure you're on a stable branch
2. List all local and remote branches to identify branches to clean up
3. Delete local branches that are no longer needed (using git branch -D)
4. Prune remote tracking branches that have been deleted on the remote (using git fetch --prune)
5. Verify the final branch state to confirm cleanup was successful

## Tools Used
- exec: Execute git commands (checkout, branch listing, branch deletion, fetch with prune)