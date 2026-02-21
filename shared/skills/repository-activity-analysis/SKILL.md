# Repository Activity Analysis
Analyze recent project activity across commits, pull requests, and contributors within a specified time window.

## When to Use
When you need to understand project momentum, contributor distribution, merge patterns, or recent development activity for a repository during a specific time period.

## Steps
1. Query recent commits using git log with date filtering and oneline format to see what was changed
2. List pull requests with state filtering to understand merge patterns and PR activity
3. Count PRs created and merged in the time window to quantify throughput
4. Count total commits in the time window to measure development velocity
5. Generate contributor statistics using git shortlog to identify who contributed most
6. Optionally inspect documentation or spec files to understand project scope

## Tools Used
- exec: Execute git commands (git log, git shortlog) to query commit history and statistics
- exec: Execute GitHub CLI (gh pr list) to query pull request metadata and states
- exec: Execute file operations (cat) to examine project documentation
