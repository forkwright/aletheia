# Multi-Layer Credential & Configuration Auditing
Systematically inspect configuration files, credentials, and source code to understand authentication setup, fallback mechanisms, and error handling for a system.

## When to Use
When troubleshooting authentication issues, understanding credential hierarchies, investigating fallback/retry logic, or auditing how a system handles provider errors and credential exhaustion.

## Steps
1. Extract and parse the main configuration file (e.g., aletheia.json) to understand default settings and model configurations
2. Inspect environment variables and PATH settings from the configuration
3. List and examine credential files to identify available credentials and their types (tokens, OAuth, etc.)
4. Search the codebase for references to backup credentials, fallback logic, and retry mechanisms
5. Locate and examine the provider/router module to understand failover logic
6. Search for error handling patterns (specific HTTP status codes like 429, 5xx, rate limiting, billing errors)
7. Cross-reference error codes with actual API responses by checking implementation details
8. Validate backup credentials and understand their refresh/refresh requirements by parsing credential details

## Tools Used
- exec: for running shell commands to cat files, parse JSON, search codebase, and examine logs
- Python inline scripts: for parsing JSON configuration and credentials
- grep: for searching relevant error handling and credential references in source code
- sed: for examining specific line ranges in source files
