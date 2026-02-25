# Module Discovery and API Integration Pattern
Locate a specific module within a large codebase, understand its structure and public API, then verify its integration points with the rest of the system.

## When to Use
When you need to understand how a particular module (e.g., a feature module, domain service, or library) is implemented and integrated into a larger system, especially in a monorepo with multiple layers (UI, runtime, infrastructure).

## Steps
1. Use find commands with path filters to locate all TypeScript files related to the target module, excluding common exclusions (node_modules, build artifacts)
2. Search for test files associated with the module to understand expected behavior and API surface
3. Use grep to search for key exported functions, types, or patterns across the codebase
4. Read the module's type definitions to understand the public interface
5. Read the module's store/data layer implementation to understand internal structure
6. Read unit tests to see usage patterns and expected behavior
7. Grep for specific method/function names to locate their implementation
8. Use sed/exec to extract specific line ranges for detailed examination
9. Search for where the module is imported/used in other parts of the system (e.g., server integration points)
10. Grep for references to the module's exported functions in orchestrators or route handlers

## Tools Used
- exec: Find files by path pattern and exclusions; extract specific line ranges; search for import statements
- grep: Search for function names, patterns, and module references; locate integration points
- read: Examine type definitions, store implementations, test files, and configuration
