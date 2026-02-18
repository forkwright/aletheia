# Session Reconstruction & Memory Synchronization
Reconstruct missing session records from database logs and synchronize operational memory with current state.

## When to Use
When a session has ended without proper logging, or when operational memory (MEMORY.md) is stale and needs to be updated with recent activity, decisions, and system state changes. Typically after discovering gaps in documentation or before critical operations that depend on accurate context.

## Steps
1. Query the session database to retrieve all messages for the session, ordered by sequence
2. Extract distillation summaries (marked with [Distillation] tags) to identify key decision points and outcomes
3. Check git history for recent commits to establish current codebase state and merged work
4. Get current date/time for timestamping reconstructed records
5. Create or update a daily memory file with reconstructed session narrative, including total message count and distillation cycles
6. Read the main operational memory file (MEMORY.md) to understand current documented state
7. Edit the operational memory file to update identity/role context if needed
8. Edit the operational memory file to update architecture and system state sections with latest merged changes and current date
9. Verify all memory updates are internally consistent with reconstructed session data

## Tools Used
- exec: query SQLite session database and git history for historical data
- write: create dated session log files with reconstructed narratives
- read: retrieve current operational memory state
- edit: update operational memory sections with current state, merged work, and timestamp
