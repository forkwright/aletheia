# Project Documentation Discovery and Retrieval
Locate and read project metadata and planning documents from a file-backed project management system.

## When to Use
When you need to understand the current state, goals, requirements, and discussion points of a project stored in a hierarchical file structure, especially when the exact file paths are not initially known.

## Steps
1. Query the project status using the plan_execute tool to get the project ID and active phases
2. Attempt to read from the expected default project directory structure
3. If files are not found at the expected location, use a broad file search with wildcards to locate the actual project directory
4. Once the correct directory is located, sequentially read key project documents in order: PROJECT.md (metadata), ROADMAP.md (goals and phases), REQUIREMENTS.md (detailed requirements), and phase-specific DISCUSS.md files (decision points and options)
5. Compile the gathered information to understand project context

## Tools Used
- plan_execute: Check project status and retrieve metadata about active phases
- exec: Search for project files using find commands with path patterns when initial paths fail
- read: Retrieve and display the contents of project documentation files
