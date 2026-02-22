# Multi-Agent Connectivity and Status Check
Verify availability and responsiveness of multiple agents in a distributed system, with fallback handling for unavailable agents.

## When to Use
When you need to confirm that multiple agents are running and responsive, troubleshoot agent availability issues, or validate system connectivity across a network of AI agents before delegating tasks.

## Steps
1. Execute a local command to verify your own connectivity (e.g., hostname check)
2. Attempt to reach the first agent using sessions_ask with a simple query
3. Handle failure gracefully when an agent is unavailable (note the error)
4. Try contacting a different agent with a lightweight ping/confirmation request
5. Send a notification to another agent acknowledging the connectivity test

## Tools Used
- exec: Local command execution to verify baseline connectivity
- sessions_ask: Query agents with timeout to check responsiveness and get confirmation
- sessions_send: One-way message to agents for status notifications without requiring responses
