# Contextualize Project State from Distributed Memory Files
Retrieve and synthesize current project context from memory files, git history, and knowledge bases to establish operational awareness.

## When to Use
When resuming work on a project, you need to quickly understand the current state, recent changes, active branches, and contextual information spread across memory systems and version control.

## Steps
1. Read the main memory/context file to understand the project identity and structure
2. Check for today's dated memory file; if unavailable, list recent memory files
3. Attempt to use any specialized context assembly tool (with graceful fallback)
4. Read the most recent dated memory file to understand recent session context
5. Check git log to see recent commits and merge activity
6. Check current git branch and status to identify active work
7. Search knowledge/memory systems for specific project-related context
8. Cross-reference git history on the active branch for task-specific timeline

## Tools Used
- exec: for reading files, listing directories, running git commands, and checking branch status
- mem0_search: for querying structured knowledge bases and memory systems about project state and plans
