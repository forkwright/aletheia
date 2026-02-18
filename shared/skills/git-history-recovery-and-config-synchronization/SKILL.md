# Git History Recovery and Config Synchronization

Restore specific files from previous git commits, update configuration templates, and commit changes with proper authorship metadata.

## When to Use
When you need to recover deleted or modified files from git history, synchronize them with current configuration templates, and preserve proper authorship information in commit history.

## Steps
1. Use `exec` with `git show HEAD~N:path/to/file` to examine files from previous commits
2. Use `read` to review the current state of configuration or template files
3. Use `edit` to update configuration templates with recovered content or corrections
4. Use `exec` with `git checkout HEAD~N -- path/to/file` to restore specific files from previous commits
5. Use `exec` with `git add -A` and `git commit` with GIT_AUTHOR_NAME and GIT_AUTHOR_EMAIL environment variables to commit with proper authorship
6. Use `exec` with `git push origin branch` to push changes to remote repository

## Tools Used
- exec: for git operations (show, checkout, add, commit, push)
- read: for reviewing current file contents before making changes
- edit: for updating configuration files with recovered or modified content
