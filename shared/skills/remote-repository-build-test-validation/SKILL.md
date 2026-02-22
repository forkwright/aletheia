# Remote Repository Build & Test Validation
Diagnose, clone, build, and systematically test a remote .NET repository to identify root causes of failures.

## When to Use
When you need to verify a remote project's build status and test suite health, especially when the repository location is unknown or the project hasn't been cloned yet. Useful for pre-deployment verification, CI/CD diagnostics, or assessing code quality before integration.

## Steps
1. SSH into the remote host and attempt to locate the repository in common directories (/tmp, /home/*)
2. If not found, clone the repository from source control into a working directory
3. Run dependency restore and full build, capturing output to identify compilation errors
4. Execute the test suite with minimal verbosity to get a high-level pass/fail summary
5. Document findings (build status, test counts, failure patterns) in a persistent note
6. Run targeted test subsets (by namespace/category) to isolate root causes of failures
7. Record session results and error analysis in a dated memory/log file

## Tools Used
- exec: SSH remote command execution with timeout and output tail filtering
- note: Documenting build/test status and identified issues for task tracking
- write: Recording detailed session findings and error patterns to a dated log file
