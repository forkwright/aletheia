# Workspace Audit and Reorganization

Systematically audit a project workspace, identify organizational issues, and restructure files/metadata for clarity and domain ownership.

## When to Use
When a workspace has accumulated mixed concerns, stale files, unclear ownership domains, or when you need to establish clear project structure and goals before proceeding with active work.

## Steps
1. Navigate to and list the root directory to understand overall structure
2. Explore subdirectories (memory, logs, scripts, specs) to map content organization
3. Check git status to identify uncommitted changes and recent activity
4. Audit for stale files (by modification date and relevance) that can be archived
5. Identify misplaced files that belong to other domains and flag for relocation
6. Scan strategic documents (BACKLOG, GOALS) to identify outdated information
7. Check availability of external tools and services the workspace depends on
8. Create archive directories for completed/stale work
9. Create domain-specific subdirectories for overflow or misplaced files
10. Update strategic documents (BACKLOG.md, GOALS.md) to reflect current priorities and ownership
11. Stage all changes and review with git status
12. Commit with a descriptive message explaining structural changes
13. Verify final state with directory listing to confirm cleanup

## Tools Used
- exec: Navigation, file listing, git operations, file movements, and document updates
- ls/find: Discovering file organization and identifying stale content
- git: Tracking changes and committing restructuring work
- cat: Reviewing and updating strategic documents
- mkdir: Creating archive and organization subdirectories
