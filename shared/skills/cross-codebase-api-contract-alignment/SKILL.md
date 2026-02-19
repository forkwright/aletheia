# Cross-codebase API Contract Alignment
Identify and propagate API parameter changes across dependent modules and tools.

## When to Use
When a core utility or interface expands its parameters (e.g., adding an optional parameter), and you need to find all call sites that should be updated to use the new capability for consistency and feature completeness.

## Steps
1. Read the utility/module definition to understand the new parameter and its purpose
2. Grep for all usages of that utility across the codebase to identify call sites
3. Grep for related configuration or context definitions that might provide the new parameter value
4. Read related files to understand how the parameter is initialized in different contexts
5. Read dependent tool implementations to see current usage patterns
6. Create a feature branch for the changes
7. Update each call site to pass the new parameter from its context
8. Run existing test suite for the modified utilities to verify compatibility
9. Run type checker to catch any contract violations
10. Commit changes with clear explanation of why the parameter is now being propagated
11. Create a PR documenting the fix

## Tools Used
- read: inspect utility definitions and call sites
- grep: locate all usages of a function/parameter across codebase
- exec: run sed for bulk replacements, execute tests, run type checker, manage git
- Git workflow: branch, diff, commit, and PR creation for tracking changes
