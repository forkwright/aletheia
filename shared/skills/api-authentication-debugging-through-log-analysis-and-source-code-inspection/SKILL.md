# API Authentication Debugging Through Log Analysis and Source Code Inspection

Systematically diagnose authentication failures in web services by correlating API logs, source code inspection, and database queries.

## When to Use
When an API service is returning authentication errors (401/403) or unexpected responses, and you need to:
- Identify whether the root cause is in authentication logic, token validation, or request handling
- Trace the actual request path through logs and code
- Verify data availability and file system state
- Distinguish between symptom (bad output) and root cause (auth failure)

## Steps
1. Attempt to reproduce the issue with a direct API call to capture the actual error
2. Document current observations and hypotheses in a memory file
3. Search source code for the relevant endpoint handler (grep for route patterns)
4. Examine the handler implementation to understand the authentication flow
5. Query the database to verify data exists and inspect path/format information
6. Check file system state to confirm resources are accessible
7. Inspect application logs filtered for auth-related keywords (Bearer, 401, 403, challenged, unauthorized, token)
8. Examine the authentication handler implementation (JwtAuthenticationHandler, etc.)
9. Correlate log patterns with code logic to identify the specific failure point
10. Document root cause findings with evidence linking symptoms to underlying mechanism

## Tools Used
- exec: run curl requests, grep source code, check logs and file system, query database
- write: record observations, hypotheses, and root cause findings to memory files
- note: capture key insights for context
