# Git Repository History and PR Analysis
Retrieve recent commits and pull requests from a Git repository to understand recent changes and development activity.

## When to Use
When you need to understand what work has been done recently on a project, track which features were merged, identify active development periods, or get context about recent commits with author and timing information.

## Steps
1. Navigate to the repository directory
2. Attempt to fetch git log with date range filtering (with fallback if date range syntax fails)
3. Query pull request list from GitHub CLI to see merged features and their timestamps
4. Retrieve detailed git log with formatted output including commit hash, message, author, and relative time
5. Parse and present the combined information about recent activity

## Tools Used
- exec: Execute git commands (git log with various filters and formats) and GitHub CLI commands (gh pr list) to retrieve repository history and pull request data