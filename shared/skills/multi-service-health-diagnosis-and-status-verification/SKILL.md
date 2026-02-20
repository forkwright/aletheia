# Multi-Service Health Diagnosis and Status Verification

Systematically probe multiple services across different ports to verify system health, identify endpoints, diagnose failures, and inspect operational logs and source code.

## When to Use
When you need to troubleshoot a distributed system with multiple microservices, verify that all components are running correctly, identify which services are accessible, and gather diagnostic information about system state and errors.

## Steps
1. Probe health check endpoints on known service ports using curl
2. Analyze health check responses to identify which services are operational
3. Attempt to access service-specific status/API endpoints to detect authorization or configuration issues
4. Query system logs (journalctl) for the relevant service to identify runtime errors or warnings
5. Search source code for relevant function names or error patterns to understand implementation
6. Test actual service functionality with sample API calls to verify end-to-end operation

## Tools Used
- exec: Used to run curl commands for HTTP health checks, grep for searching code patterns, and journalctl for accessing system logs