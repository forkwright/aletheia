# Feature Branch Development and Deployment with Git Integration

Implement a complete feature development workflow including local edits, git management, remote builds, and pull request creation.

## When to Use
When developing a feature branch that requires:
- Code modifications across multiple files
- Version control commits to a feature branch
- Remote server builds/validation
- Pull request creation for code review
- Bug fixes discovered during validation

## Steps
1. Edit source files to implement feature changes
2. Stage all changes with git add
3. Commit changes with descriptive message including feature scope
4. Fetch and checkout feature branch on remote build server
5. Execute remote build to validate compilation
6. Review build output and identify any issues
7. Make targeted fixes to problematic files locally
8. Commit fixes and push to remote feature branch
9. Pull latest changes on remote server and rebuild to validate
10. Create pull request with comprehensive title and description

## Tools Used
- edit: Modify source code files with specific text replacements
- exec: Execute git commands (add, commit, push, pull) and remote SSH builds
- ssh: Execute remote compilation and validation on build server
- gh: Create pull requests with detailed descriptions
