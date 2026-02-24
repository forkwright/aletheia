# Service Health and Activity Verification
Verify the operational status and recent activity of a system service by checking process state, file modifications, and logs.

## When to Use
When you need to confirm that a service is running, assess its recent activity, identify when it last made changes, and verify there are no error conditions in its logs.

## Steps
1. Check if the service process is currently running using process listing (ps aux with grep filtering)
2. Verify the timestamp of the service's primary output/state file to confirm recent activity
3. Retrieve the most recent log entries from the service's log files (checking multiple potential log locations)
4. Filter logs for error conditions (errors, exceptions, tracebacks) to confirm clean operation

## Tools Used
- exec: Execute shell commands to query process status, file metadata, and system logs