# Skill Name
Diagnose and validate credential lifecycle health by tracing credential files, monitoring systems, and reconciling discrepancies.

## When to Use
When an alert flags potentially stale or expired credentials, and you need to determine whether the issue is a false positive (old backup file) or genuine problem (primary credential actually failing). Useful for validating credential refresh automation is working correctly.

## Steps
1. Read the alert/status file to identify the specific credential concern and priority level
2. Search memory for context on the credential refresh process and any known issues
3. Attempt to read credential configuration files directly to understand the system setup
4. Search cron jobs and systemd timers for automated credential refresh processes
5. Examine the credential refresh script to understand its logic and refresh mechanism
6. Check refresh logs to see if the process is executing and token is actually being renewed
7. Locate and list all credential files in the system to map the credential landscape
8. Run credential status command to get current token expiry information
9. Inspect credential files (sanitizing sensitive data) to understand structure and metadata
10. Trace multiple credential locations to identify which are primary vs. backup/stale
11. Check file modification timestamps to determine freshness of each credential file
12. Review monitoring configuration to understand what thresholds triggered the alert
13. Search codebase for references to backup credential handling and failover logic
14. Check whether backup credentials are actually configured or registered in the system
15. Verify through code inspection that stale backup files aren't being used
16. Document findings and retract inaccurate memory items that conflate stale files with actual failures

## Tools Used
- read: Initial alert/status retrieval
- mem0_search: Find context on credential refresh patterns
- exec: Run shell commands to inspect files, check cron/systemd, get status
- grep: Search codebase for credential handling logic
- note: Document findings and reconciliation
- mem0_retract: Remove inaccurate memory entries about stale credentials

---