# Batch Merge Pull Requests with Squash and Auto-Cleanup

Merge multiple pull requests sequentially using squash commits with custom messages, then automatically delete their branches.

## When to Use
When you need to merge several related or independent pull requests into a main branch while:
- Keeping commit history clean via squash merging
- Applying consistent or custom commit messages
- Automatically cleaning up feature branches after merge
- Working in a git repository with GitHub CLI access

## Steps
1. Navigate to the repository root directory
2. For each pull request to merge:
   - Use `gh pr merge` with the PR number
   - Apply `--squash` flag to combine commits
   - Use `--subject` to provide a custom commit message (typically following conventional commit format)
   - Add `--delete-branch` to automatically remove the source branch after merge
3. Verify merge completion by checking the output for branch updates and fast-forward status

## Tools Used
- exec: Execute shell commands to run GitHub CLI merge operations and navigate directories
