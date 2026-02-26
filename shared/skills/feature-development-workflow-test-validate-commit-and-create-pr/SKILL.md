# Feature Development Workflow: Test, Validate, Commit, and Create PR

Complete workflow for developing a feature: run tests, update exports, validate TypeScript, commit changes, and create a pull request.

## When to Use
When implementing a feature in a TypeScript/Node.js codebase that requires:
- Running test suites to verify functionality
- Updating module exports
- Type-checking the codebase
- Committing changes to git
- Creating a pull request on GitHub

## Steps
1. Run the specific test file for the feature to verify it passes
2. Update the index/export file to expose new functionality
3. Run TypeScript type checker to catch compilation errors
4. Run related test suites to ensure no regressions
5. Stage and commit the changes with a descriptive commit message
6. Push the feature branch to origin
7. Create a pull request with a clear title and description

## Tools Used
- exec: Running test suites (vitest), TypeScript compiler (tsc), and git commands (add, commit, push)
- edit: Updating module exports and index files
- GitHub CLI (gh): Creating pull requests programmatically
