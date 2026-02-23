# Feature Implementation with Validation and Deployment Pipeline

Complete workflow for implementing a feature change, validating it, merging to main, and deploying to production.

## When to Use
This pattern applies when you need to implement a feature in a web application, ensure it passes tests and builds successfully, create a pull request, merge it, and deploy the changes to a production environment.

## Steps
1. Read the target file to understand current implementation
2. Search related files (stores, API clients, configs) to identify integration points and dependencies
3. Modify the target file with the feature implementation
4. Build the project and run test suite to validate changes
5. Create a feature branch and commit changes with descriptive message
6. Push branch and create a pull request with title and description
7. Merge the pull request using squash commit
8. Switch back to main branch, pull latest changes, and clean up feature branch locally and remotely
9. Rebuild the project and deploy artifacts to production directory
10. Verify deployment by testing the deployed application (health checks, UI verification)

## Tools Used
- read: examine current file implementation
- exec: search files for context, run build/test commands, manage git workflow, and verify deployment
- write: update files with new feature implementation
