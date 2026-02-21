# Session Context Recovery and Project Status Check
Retrieve historical session notes and current project state to understand recent work and ongoing tasks.

## When to Use
At the start of a work session to understand what has been done recently, what the current project state is, and what tasks may be in progress.

## Steps
1. Check for today's daily memory file; if not found, note the absence
2. Retrieve yesterday's daily memory file to understand recent completed work
3. List open pull requests to identify in-progress work
4. View recent git commits to understand the project's development trajectory
5. Synthesize this information to establish current context

## Tools Used
- exec: to run shell commands for file retrieval (cat), date checking, git operations (gh pr list, git log), and conditional error handling