# Codebase Reconnaissance & Audit Pattern
Systematically explore a software project's structure, documentation, and code to understand architecture, identify quality issues, and plan improvements.

## When to Use
When starting work on an unfamiliar codebase, conducting a code quality audit, or gathering information before proposing refactoring/improvement PRs.

## Steps
1. List top-level directory structure to understand project layout
2. Search for source files by extension (`.ts`, `.tsx`, `.js`, `.svelte`) while excluding build artifacts and dependencies
3. Read key documentation files (README, CONTRIBUTING, project-specific guides like CLAUDE.md)
4. Examine git metadata (remotes, branches) to understand version control state
5. Explore directory structure of main source directories (infrastructure/runtime/src, ui/src, etc.)
6. Review configuration and styling files (package.json, global CSS, index.html)
7. Examine component/module structure and key entry points (App.svelte, main server files)
8. Sample critical components and shared utilities to understand patterns
9. Check error handling and type definitions architecture
10. Review documentation structure and quickstart guides
11. Sample recent git history for context on recent changes
12. Check version and release metadata
13. Document findings in structured notes identifying specific drift areas or quality issues
14. Synthesize findings into a comprehensive improvement plan with prioritized areas

## Tools Used
- exec: Running find commands to locate files by type/pattern, reading file contents with cat, checking git history and metadata
- ls: Examining directory listings with timestamps and sizes
- note: Capturing audit findings and task items for reference
- plan_propose: Synthesizing observations into a structured improvement plan with prioritized steps
