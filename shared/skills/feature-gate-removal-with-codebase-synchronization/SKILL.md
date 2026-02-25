# Feature Gate Removal with Codebase Synchronization
Remove a hardcoded limit from a codebase by updating all dependent references, validating changes, and committing.

## When to Use
When you need to eliminate a specific hardcoded constraint (like a cap or limit) that appears across multiple files in a codebase, and you need to ensure all references are updated consistently without breaking the build.

## Steps
1. Search for all occurrences of the constraint identifier (e.g., `MAX_NOTES_PER_SESSION`) across the codebase using grep
2. Read the relevant files to understand how the constraint is used and referenced
3. Use sed or similar tools to pinpoint exact sections where the limit is enforced
4. Edit each file to remove the constant definition
5. Edit all locations where the constant is referenced in logic or documentation
6. Run the project's type checker or compiler to validate there are no build errors
7. Read the modified files one final time to confirm the changes are correct
8. Commit the changes with a descriptive message using git

## Tools Used
- read: Examine file contents and understand constraint usage patterns
- grep: Locate all instances of the constraint identifier across files
- exec: Run sed to find exact line numbers and the compiler to validate changes
- edit: Remove constant definitions and update all references systematically
- git: Commit validated changes to version control
