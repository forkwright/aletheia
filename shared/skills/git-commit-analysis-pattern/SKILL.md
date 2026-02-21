# Git Commit Analysis Pattern
Analyze the most recent commit to understand what changes were made and their scope.

## When to Use
When you need to understand what a recent code change accomplished, review the files affected by the latest commit, or examine the specific code modifications in the most recent change.

## Steps
1. View recent commit history with git log to identify the latest commit and its message
2. Check which files were modified using git diff HEAD~1 --stat to see file-level changes
3. Review the full diff using git diff HEAD~1 to examine the actual code changes line-by-line

## Tools Used
- exec: Running git commands to access version control information