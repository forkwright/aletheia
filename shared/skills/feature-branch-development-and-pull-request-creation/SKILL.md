# Feature Branch Development and Pull Request Creation

Automate the workflow of committing changes, pushing to a remote branch, building/testing on a remote server, and creating a pull request.

## When to Use
When you need to complete a full feature development cycle: commit local changes, push to a feature branch, verify the build on a remote environment, and open a pull request for code review.

## Steps
1. Commit changes to a local feature branch with a descriptive commit message
2. Push the feature branch to the remote repository
3. SSH into a remote server to fetch the latest changes and verify the build compiles successfully
4. Create a pull request with a title and description summarizing the changes
5. Verify the PR link is generated and accessible

## Tools Used
- exec: for running git commands (commit, push), SSH connections, and remote builds (dotnet build)
- gh (GitHub CLI): for creating pull requests with structured metadata