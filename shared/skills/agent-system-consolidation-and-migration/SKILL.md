# Agent System Consolidation and Migration

Safely consolidate multiple agent workspaces into a primary agent while preserving valuable content and maintaining system integrity.

## When to Use
When decommissioning or folding multiple specialized agents into a primary agent, and you need to:
- Preserve institutional knowledge and work products
- Audit what each agent has accomplished
- Migrate domain-specific content to the primary system
- Update routing logic and memory records
- Create an audit trail of the consolidation

## Steps
1. Load and inspect the central configuration file (JSON) to understand current agent setup, workspaces, and bindings
2. Parse agent list to identify all active agents and their workspace locations
3. Check agent bindings to signal channels and communication networks
4. Inspect each agent's workspace directory structure and key documentation (MEMORY.md, IDENTITY.md, etc.)
5. Review each agent's memory logs and domain-specific knowledge files
6. Query the sessions database to confirm active sessions and message history for each agent
7. Create destination directories in primary agent's workspace
8. Copy valuable domain-specific content (cases, knowledge, memory logs) to primary workspace
9. Flag decommissioned agent workspaces as archived with timestamp and rationale
10. Update routing delegation rules in AGENTS.md to reflect consolidated responsibilities
11. Update primary agent's MEMORY.md with consolidated task backlog and context
12. Write a consolidation log entry documenting the decision and what was preserved

## Tools Used
- exec: shell commands for inspecting config files, listing directories, and performing file operations
- read: reviewing policy and configuration markdown files
- edit: updating delegation routing rules and memory records
- write: creating consolidation session logs and audit records
- sqlite3: querying active sessions database to understand agent activity patterns
