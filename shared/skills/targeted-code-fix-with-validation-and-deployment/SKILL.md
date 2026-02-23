# Targeted Code Fix with Validation and Deployment

Fix specific bugs in a codebase by locating relevant code, making targeted edits, validating changes, and deploying.

## When to Use
When you need to fix identified bugs in a web application, make changes across multiple related files, verify the fix doesn't break existing functionality, and deploy the updated code to production.

## Steps
1. Search the codebase to locate all files related to the bug (using grep/find with specific patterns)
2. Read the relevant files to understand the current implementation
3. Make targeted edits to fix the identified issues across multiple files
4. Build the project to ensure compilation succeeds
5. Run the test suite to verify no regressions were introduced
6. Commit the changes with a descriptive message explaining the fixes
7. Deploy the built artifacts to the target environment

## Tools Used
- exec: search codebase with grep/find, build project, run tests, commit changes, deploy files
- read: examine file contents to understand current implementation
- edit: make targeted fixes to specific code sections