# Feature Development & Code Review Workflow

Orchestrate a complete feature development cycle: merge prior work, investigate requirements, create feature branch, implement multi-file changes, test, fix compilation errors, commit, push, and create pull request.

## When to Use
When developing a new feature that requires:
- Multiple related source files and corresponding test files
- Investigation of existing codebase patterns and requirements
- TypeScript compilation and test validation
- Git workflow management (branch creation, commits, pushes, PR creation)
- Coordination across several interdependent modules

## Steps
1. Merge completed prior work via pull request
2. Query requirements/specifications to understand scope
3. Create feature branch for new work
4. Analyze existing related code (line counts, patterns, key exports)
5. Search codebase for relevant patterns and existing implementations
6. Create source implementation files with full content
7. Create corresponding test files
8. Document progress in notes and state files
9. Update index/export files to expose new modules
10. Run TypeScript compiler to catch type errors
11. Identify and fix unused imports and parameters
12. Verify compilation succeeds
13. Stage all changes with git add
14. Commit with descriptive message
15. Push feature branch and create pull request via GitHub CLI

## Tools Used
- exec: run git commands, grep searches, TypeScript compilation, test execution
- write: create new implementation and test files, update documentation
- edit: fix compilation errors, update exports
- note: track progress and phase completion
