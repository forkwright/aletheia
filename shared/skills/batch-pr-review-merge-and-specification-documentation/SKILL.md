# Batch PR Review, Merge, and Specification Documentation

Systematically review open pull requests, verify mergeability, merge them in sequence with standardized commit messages, then create comprehensive specification documentation.

## When to Use
When you need to:
- Consolidate multiple related PRs that have been vetted and are ready to merge
- Ensure all PRs are mergeable before proceeding
- Merge PRs with consistent, descriptive squash commit messages
- Document the completed work as a formal specification
- Verify the repository state after batch operations

## Steps
1. List all open PRs to identify candidates for merge
2. View details (title, body, file changes, additions/deletions) for each PR to understand scope
3. Check mergeability status for all candidate PRs simultaneously
4. Merge each PR in dependency order using `--squash` flag with descriptive body text including rationale and post-merge actions
5. Verify merge success by pulling latest main branch and confirming no open PRs remain
6. Explore the codebase (UI components, styles, stores, utilities) to understand the merged changes
7. Review existing specification structure to match documentation patterns
8. Write a comprehensive specification document covering: overview, phases, technical details, affected systems, and implementation notes
9. Commit the specification document with descriptive message

## Tools Used
- exec: run gh CLI commands to list/view/merge PRs, manage git branches
- read: examine source files (Svelte components, TypeScript, CSS) to understand merged changes
- write: create specification documentation
- web_fetch: research external context if needed