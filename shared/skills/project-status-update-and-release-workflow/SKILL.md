# Project Status Update and Release Workflow
Batch update project documentation, commit changes, and create a pull request to release completed work.

## When to Use
When you've completed a phase of work across multiple files and need to:
- Update specification/roadmap documents to reflect completion status
- Commit all changes with a comprehensive message
- Push to a feature branch
- Create a pull request for code review

## Steps
1. Review current git status to understand scope of changes
2. Identify all documentation files that need status updates (specs, roadmap, etc.)
3. Use Python string replacement to safely update multiple markdown files with new statuses and checkboxes
4. Stage all modified files with git add
5. Create a detailed commit message summarizing the work completed across specs/phases
6. Push the feature branch to remote
7. Create a pull request with the same summary information using gh CLI

## Tools Used
- note: retrieve task context and requirements
- exec: run git commands, file discovery, and Python scripts for bulk documentation updates
- exec: execute git commit, push, and gh pr create for release workflow
