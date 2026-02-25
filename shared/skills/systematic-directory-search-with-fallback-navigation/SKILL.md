# Systematic Directory Search with Fallback Navigation

Locate a target directory or file collection when the expected path doesn't exist, using progressively broader search strategies.

## When to Use
When you need to find a specific directory or set of files (like specs, docs, configs) but the initial path is incorrect or unknown. Useful when working in unfamiliar codebases or after directory structure changes.

## Steps
1. Attempt direct access to the expected path
2. Use find() with specific naming patterns (lowercase, uppercase variants)
3. When narrower searches fail, use ls -la to inspect the actual directory structure
4. Once a parent directory is located, navigate into it and refine the search
5. Combine multiple search criteria in a single find command (names, paths, extensions)
6. Use head/limit to manage output when multiple matches exist
7. Read the README or index file to confirm you've found the right location

## Tools Used
- exec: Execute directory navigation (cd) and list commands (ls -la) to understand structure
- find: Search with various pattern combinations (case variations, wildcards, path filters)
- read: Verify the correct directory by examining README or documentation files
