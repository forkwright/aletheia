# Alert Escalation Pattern
Monitor a status file for critical items, verify system health, and escalate unresolved issues to another agent.

## When to Use
When you need to track overdue or critical tasks in a shared status document, ensure the monitoring system is operational, and notify another agent about items that have reappeared or remained unresolved after previous attempts.

## Steps
1. Read the alert/status file to identify critical or overdue items
2. Verify the monitoring system is active and functioning
3. Compose an escalation message that includes context about the items and their status
4. Send the escalation message to the appropriate agent for follow-up action

## Tools Used
- read: to retrieve current critical items from the status file
- exec: to verify system health/status of the monitoring service
- sessions_send: to escalate the alert to another agent for action