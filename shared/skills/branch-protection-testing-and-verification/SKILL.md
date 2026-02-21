# Branch Protection Testing and Verification
Verify that branch protection rules are correctly applied by creating a test branch, attempting to commit changes, and then cleaning up test artifacts.

## When to Use
When you need to validate that branch protection settings (such as requiring pull requests, blocking direct pushes) are properly enforced on a protected branch like main.

## Steps
1. Configure branch protection rules using the GitHub API (PUT request to branch protection endpoint)
2. Create a test branch to attempt circumventing the protection
3. Make a test commit on the test branch to establish a divergent state
4. Verify that the protection rules block direct pushes to the protected branch
5. Clean up by resetting the repository to its original state
6. Delete the test branch

## Tools Used
- exec: Execute shell commands for git operations and GitHub API calls
- gh api: Configure branch protection settings via GitHub CLI
- git checkout: Create and switch to test branches
- git commit: Create test commits
- git reset: Restore repository to pre-test state
- git branch: Delete temporary test branches