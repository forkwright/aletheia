# Repository Discovery and Assessment
Systematically gather comprehensive information about a Git repository's structure, status, configuration, and development workflow.

## When to Use
When you need to understand an unfamiliar codebase, assess project health, identify active development areas, or gather context before making changes to a repository.

## Steps
1. Fetch latest remote data and list open pull requests to understand active development
2. Review recent commit history to identify recent changes and development direction
3. Map repository structure, excluding common build/dependency directories (`.git`, `node_modules`)
4. Count total files to gauge project size and complexity
5. Read key documentation files (README, ROADMAP, CONTRIBUTING) to understand project goals and structure
6. Examine GitHub configuration (workflows, issue templates, security settings) for development practices
7. Check documentation for performance baselines, security audits, and development guides
8. List open issues and their labels to understand backlog and prioritization
9. Compare main and development branches to identify pending changes
10. Document findings for planning next actions

## Tools Used
- exec: Execute git commands (fetch, log, branch), file system inspection, and gh CLI queries
- git commands: fetch, log, branch listing, commit history comparison
- gh CLI: pull request and issue listing with JSON output for structured data
- find: Recursive file discovery with exclusion patterns
- cat: Read documentation and configuration files
- plan_propose: Propose next steps based on gathered intelligence
