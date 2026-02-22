# Code Audit with Cross-Reference Validation
Identify bugs by comparing code references against a source-of-truth database schema, then systematically fix all discrepancies.

## When to Use
When auditing a codebase for references to entities (tables, classes, methods) that may no longer exist or have been renamed, requiring systematic discovery and bulk remediation across multiple files.

## Steps
1. Pull latest changes and create a feature branch for fixes
2. Identify the category of potential issues (e.g., obsolete table references)
3. Extract the source-of-truth list (e.g., all tables that exist in migrations)
4. Scan codebase for references to that category across all relevant files
5. Compare references against source-of-truth to identify missing/invalid entities
6. For each invalid reference, locate all usages and files affected
7. Understand the intended behavior of affected code sections
8. Rewrite affected code to use valid alternatives (e.g., new schema)
9. Stage, commit, and push fixes with detailed commit messages
10. Create pull request documenting all changes
11. Verify build success on remote
12. Update audit documentation to mark issues as resolved
13. Create follow-up PR for documentation

## Tools Used
- exec: Run git commands, grep for code references, extract schema definitions from migrations
- read: Examine file contents to understand context before rewriting
- write: Create new filter/helper classes and update audit documentation
- edit: Modify existing files to apply fixes (add attributes, fix qualified names, adjust method calls)
- note: Track progress and maintain state across long sessions
