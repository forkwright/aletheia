# Project Workspace Audit
Comprehensively assess a project directory's structure, storage usage, configuration status, and version control history.

## When to Use
When you need to quickly understand the state of a project workspace, including its size distribution, configuration files, git status, and recent changes. Useful for onboarding, diagnostics, or pre-deployment checks.

## Steps
1. Measure disk usage across all major directories using `du -sh` to identify large directories and storage distribution
2. Check for configuration files and database artifacts in hidden directories (`.summus/`, `.env.*`) to understand project setup
3. Verify git remote configuration to confirm repository origin and connectivity
4. Review recent git commits to understand recent work and project momentum

## Tools Used
- exec: executes shell commands to query filesystem, configuration, and git information