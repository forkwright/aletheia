# Feature Branch Testing and Pull Request Creation Workflow
Execute tests on a feature branch, stage changes, commit, push, and create a pull request with automated messaging.

## When to Use
When you need to complete a full development workflow: verify code changes with tests, commit modifications to a feature branch, push to remote, and create a pull request for code review.

## Steps
1. Run tests on the feature branch to verify code changes
2. Check git diff to review what files were modified
3. Check git status to see all staged/unstaged changes
4. Stage and commit the modified files with a descriptive commit message
5. Push the feature branch to the remote repository
6. Create a pull request using gh CLI with title and body description

## Tools Used
- exec: Running test suites, git commands, and GitHub CLI operations
- vitest: For executing unit tests on specific test files
- git: For version control operations (diff, status, add, commit, push)
- gh: For creating pull requests with structured metadata
