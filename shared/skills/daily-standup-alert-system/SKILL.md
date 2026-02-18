# Daily Standup Alert System
Aggregate pending tasks from a local file, check system health, and send a consolidated status alert to a monitoring agent.

## When to Use
When you need to create an automated daily check-in workflow that:
- Reviews a local task/priority file for pending items
- Verifies system health and configuration
- Notifies a monitoring or coordination agent of overdue/urgent items
- Runs on a schedule or as part of a startup/check-in routine

## Steps
1. Read the task/priority tracking file to identify pending and overdue items
2. Execute diagnostic commands to check tool availability and system health
3. Attempt to gather additional context (calendar, diagnostics) with graceful fallbacks for unavailable tools
4. Format collected information (tasks + system status) into a summary message
5. Send the consolidated alert to a designated monitoring agent

## Tools Used
- read: to retrieve pending tasks and priorities from a tracked file
- exec: to run diagnostic commands and check tool availability with error handling
- sessions_send: to dispatch the formatted alert to a monitoring/coordination agent
