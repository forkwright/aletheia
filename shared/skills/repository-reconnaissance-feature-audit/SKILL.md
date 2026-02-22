# Repository Reconnaissance & Feature Audit
Systematically discover project structure, recent changes, active specs, and in-flight work across a codebase.

## When to Use
When you need to quickly understand a repository's current state, ongoing development priorities, architectural layout, and what features are being built—especially useful for onboarding to a new project, assessing scope before making contributions, or tracking project momentum.

## Steps
1. Locate the repository (check local paths and remote servers via SSH)
2. Extract recent commit history with messages to understand latest work
3. Review ROADMAP and spec files to identify active priorities and phases
4. List directory structure (pages, components, stores, services) to understand architecture
5. Examine recent commits in detail (--stat) to see which files changed
6. Sample key source files (entry points, test files) to understand tech stack
7. Query open/merged pull requests to see in-flight features and recent completions
8. Review PR bodies to understand feature scope and spec alignment
9. Check package.json and build configs (Cargo.toml, etc.) for dependencies and metadata
10. Verify local state is current (git pull)
11. Collect final inventory of all major directories and line counts

## Tools Used
- exec: Running shell commands to navigate filesystem, execute git queries, list directories, read file contents, and query GitHub CLI for PR/issue data
- git log: Extracting commit history, diffs, and change statistics
- gh (GitHub CLI): Querying pull request state, metadata, and bodies
- cat/head: Sampling source code and documentation files
