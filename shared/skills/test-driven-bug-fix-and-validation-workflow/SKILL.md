# Test-Driven Bug Fix and Validation Workflow
Diagnose test failures, fix source code and test mocks, validate with builds and type checks, then commit and create a PR.

## When to Use
When you need to fix failing tests in a TypeScript/Node.js project by identifying root causes, updating mock data across multiple test files, ensuring consistency, and validating the complete solution before merging.

## Steps
1. Build the project to establish baseline
2. Run specific test suites to identify failures
3. Use grep and sed to examine test file structure and mock definitions
4. Identify inconsistencies in mock data across related test files
5. Edit test files to synchronize mock implementations across the test suite
6. Run type checker and full test suite to validate fixes
7. Rebuild project to confirm no regressions
8. Stage changes and create a git commit with descriptive message
9. Open a pull request with the fix

## Tools Used
- exec: for running build, test, and type-checking commands; grep/sed for inspecting test files
- edit: for updating mock data in test files to maintain consistency
- git: for staging, committing, and creating pull requests
