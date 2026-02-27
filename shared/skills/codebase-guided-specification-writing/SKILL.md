# Codebase-Guided Specification Writing
Write a new technical specification by researching related code, documentation, and architectural decisions in the codebase first.

## When to Use
When you need to author a new technical specification (RFC/ADR) that must be consistent with existing system design, naming conventions, and architectural patterns. Use this when the spec proposes changes that touch multiple modules or require understanding current implementation details.

## Steps
1. Search codebase for related terms, existing specs, and architectural documentation
2. Read foundational design documents (e.g., ARCHITECTURE.md, naming philosophy docs)
3. Examine the most recent related specs to understand current status and progression
4. Inspect actual source code in relevant modules to understand current implementation
5. Trace imports and cross-references between modules to understand dependencies
6. Write the specification document with proper structure (status, problem, solution, implementation details)
7. Commit the spec to version control with descriptive message
8. Update or create related tracking issue with spec summary and phase breakdown
9. Record the work in task notes with spec number, commit hash, and phase outline

## Tools Used
- exec: for grepping documentation/code, reading files, checking directory structure, running git commands, and GitHub CLI updates
- write: for creating the specification document file
- note: for recording completion and phase breakdown in task tracking
