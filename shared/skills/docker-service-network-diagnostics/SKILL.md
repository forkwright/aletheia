# Docker Service Network Diagnostics
Diagnose and verify Docker container networking, port mappings, and service connectivity.

## When to Use
When you need to understand how Docker containers are networked, what ports they expose, which services they connect to, or troubleshoot connectivity issues between containerized services.

## Steps
1. List running containers with their status and exposed ports using `docker ps` with filters and custom format
2. Inspect container network configuration using `docker inspect` to examine port mappings and network settings
3. Query specific port information using `docker port` command for direct port mapping details
4. Check the container's network mode (bridge, host, etc.) using `docker inspect`
5. Verify actual service connectivity by making HTTP requests to the exposed endpoints and checking response codes

## Tools Used
- exec: Execute Docker CLI commands to query container state, inspect configuration, and test connectivity