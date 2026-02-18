# System Health and Context Assessment
Quickly diagnose the current state of a system by gathering operational, task, and git status information.

## When to Use
When you need to rapidly understand the baseline state of a system before taking action, including active services, pending tasks, recent changes, and system logs. Useful for pre-action verification, troubleshooting, or context gathering before executing modifications.

## Steps
1. Attempt to assemble context from the system (if available)
2. Check for recent daily logs by date
3. List available memory/log files to understand documentation state
4. Verify critical services are running (systemctl check)
5. Retrieve current task queue/pending work items
6. Get recent git history to understand recent changes
7. Check git status for uncommitted modifications
8. Review system logs for the service to identify recent issues or activity

## Tools Used
- exec: Used to run diagnostic commands including assemble-context, file inspection, systemctl, task management tools, git commands, and journalctl logging