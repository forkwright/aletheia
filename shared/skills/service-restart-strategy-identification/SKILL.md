# Service Restart Strategy Identification
Identify and attempt multiple methods to restart a service, from locating restart scripts to using systemctl commands.

## When to Use
When you need to restart a service but don't know the exact restart mechanism, or when you need to verify which restart methods are available in the system.

## Steps
1. Search for service-specific restart scripts using `which` and `ls` commands in common binary directories
2. Inspect update/deployment scripts to understand the service restart patterns they use
3. Extract restart commands (e.g., systemctl calls) from those scripts using grep with relevant keywords like "restart" and "systemctl"
4. Attempt direct API-based restart/reload if available at known endpoints (e.g., `/api/config/reload`)
5. Handle authorization errors gracefully and fall back to alternative methods if needed

## Tools Used
- exec: for executing shell commands to search for scripts, grep restart patterns, and test API endpoints