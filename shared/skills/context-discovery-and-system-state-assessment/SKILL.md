# Context Discovery and System State Assessment
Systematically locate and read configuration files, logs, and system state to understand the current environment and available tools.

## When to Use
When entering an unfamiliar system environment, needing to understand available tools, recent work, and current project state before taking action.

## Steps
1. Sample key configuration/documentation files in expected locations (e.g., README, main docs)
2. Attempt to use expected tools; if they fail, locate them via which/find commands
3. Check environment variables (e.g., $PATH) to understand system setup
4. Read recent logs or daily notes to understand ongoing work and open issues
5. Check version control status and recent commits to understand project state
6. List available tools/binaries in key directories to inventory capabilities

## Tools Used
- exec: for running shell commands to probe file system, check tools, read logs, and query git history
