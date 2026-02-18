# Authenticate and Merge GitHub Pull Request
Verify git/GitHub authentication status, merge a pull request with a custom message, confirm merge completion, and sync local repository.

## When to Use
When you need to merge a GitHub pull request programmatically as part of an automated workflow, and want to ensure proper authentication and local sync afterward.

## Steps
1. Verify git remote configuration and authentication setup (check credential helper and git config)
2. Confirm GitHub CLI authentication status and active account
3. Merge the pull request using `gh pr merge` with a descriptive commit message
4. Verify the pull request state changed to MERGED by querying its metadata
5. Pull the merged changes from the remote main branch to update local repository

## Tools Used
- exec: Execute shell commands to run git and GitHub CLI operations
- gh auth status: Verify GitHub CLI authentication
- gh pr merge: Merge pull request with custom message
- gh pr view: Query pull request state
- git pull: Sync local repository with remote changes