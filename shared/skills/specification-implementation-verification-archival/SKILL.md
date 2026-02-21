# Specification Implementation Verification & Archival
Systematically verify that a design specification has been fully implemented in code, then archive the spec and document the completion.

## When to Use
When a design specification document is complete and you need to:
- Confirm all specified features/modules are implemented in the codebase
- Track which files and components fulfill each requirement
- Archive the spec once implementation is verified
- Update documentation to reflect the spec's completion status

## Steps
1. Read the specification document to understand its scope and requirements
2. Extract the main section headers (##) to identify key requirement areas
3. Extract subsection headers (###) to identify specific features/modules
4. For each major requirement, use grep to search the codebase for corresponding implementation files and patterns
5. Verify implementation status by checking for key classes, functions, or configuration parameters mentioned in the spec
6. Cross-reference with git history to find related commits that implemented the spec
7. Check companion documents (related specs, privacy specs) for integrated requirements
8. Move the specification document to an archive directory
9. Create or update a README documenting the specifications and their status
10. Commit the archival changes with a message referencing the PR that implemented the spec

## Tools Used
- read: to load and review the specification document
- exec (grep): to search for implementation patterns and verify code exists
- exec (git show): to cross-reference implementation commits
- exec (mv): to move spec to archive directory
- exec (cat): to create documentation files
- exec (git add/commit): to track archival in version control
