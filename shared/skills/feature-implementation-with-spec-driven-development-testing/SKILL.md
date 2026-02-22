# Feature Implementation with Spec-Driven Development & Testing
Complete workflow for implementing a feature spec through code exploration, implementation, testing, and merge into main branch.

## When to Use
When implementing a well-defined feature specification that requires:
- Understanding existing codebase architecture and patterns
- Creating new store/hook modules
- Integrating into multiple existing components
- Writing and fixing tests before merge
- Following a structured commit and PR workflow

## Steps
1. Read the feature spec to understand goals and dependencies
2. Create a feature branch (git checkout -b feat/...)
3. Grep the codebase to locate relevant existing patterns (stores, hooks, types)
4. Read key files to understand current architecture and data flow
5. Create new store/hook modules implementing the feature logic
6. Write integration tests for new modules
7. Edit existing components to import and use new functionality
8. Run build and test suite to catch errors
9. Fix compilation errors and test failures iteratively
10. Run full test suite to verify no regressions (all tests pass)
11. Git commit with descriptive message referencing the spec
12. Create PR with title, body, and spec reference
13. Merge PR via gh (squash or standard merge)
14. Check out main, pull latest, delete feature branch locally and remotely
15. Verify final state (tests, build, open PRs, commits)

## Tools Used
- exec: navigate, grep patterns, checkout branches, build, test, create/merge PRs
- read: understand existing file structure and patterns
- write: create new modules (stores, hooks, tests)
- edit: integrate new code into existing components
- note: track intermediate progress state
