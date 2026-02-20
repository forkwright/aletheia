# Fix Missing Mock Dependencies in Test Files

Systematically identify and add missing mock implementations to test setup functions to resolve failing unit tests.

## When to Use
When unit tests are failing with "is not a function" errors for mocked service dependencies, indicating incomplete mock setup in test fixture factories.

## Steps
1. Run failing tests and capture error messages mentioning undefined mock functions
2. Grep the source code to find where the missing functions are defined (e.g., in store implementations)
3. Grep test files to locate the mock factory function (e.g., makeStore())
4. View the current mock factory implementation to see what's already mocked
5. Search related test files for patterns of similar mock setups
6. Edit the mock factory to add missing mock methods for all functions called by the code under test
7. Re-run tests to verify the fixes work
8. Commit changes with clear messaging about which mocks were added
9. Create pull request documenting the fix scope

## Tools Used
- exec: grep to find function definitions and test usages, run test suites, verify builds
- edit: add missing mock method implementations to test fixture factories
