# Credential File Age Audit Pattern
Audit the age and configuration of credential files to identify expiring or stale credentials.

## When to Use
When you need to monitor credential file freshness, identify potential security risks from outdated credentials, or verify that credential files match their expected configuration.

## Steps
1. Read the credential signal/monitoring module to understand the credential checking logic and expectations
2. List credential files with metadata (ls -la) to retrieve modification timestamps and permissions
3. Query the credential configuration file to see which token files are registered and their expected locations
4. Compare actual credential file timestamps against the configuration to identify mismatches or age concerns

## Tools Used
- read: to examine the credential monitoring code and understand what should be checked
- exec: to inspect filesystem metadata (ls with timestamps) and query configuration files (grep)