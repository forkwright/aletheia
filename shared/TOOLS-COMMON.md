# Common Tools (All Agents)

## Shared bin/
All agents have access to shared tools at `/mnt/ssd/aletheia/shared/bin/`:

| Tool | Purpose |
|------|---------|
| facts | Atomic fact store (shared) |
| gcal | Google Calendar |
| gdrive | Google Drive |
| tw | Taskwarrior |
| research / pplx | Perplexity search |
| letta | Memory server |
| mcporter | MCP server interface |
| morning-brief | Daily summary |
| agent-status | Cross-agent status |

## MCP Servers (via mcporter)

| Server | Tools | Purpose |
|--------|-------|---------|
| memory | 9 | Knowledge graph (entities, relations, observations) |
| github | 26 | GitHub API |
| sequential-thinking | 1 | CoT reasoning |
| task-orchestrator | 7 | Hierarchical tasks |

**Usage:**
```bash
mcporter list <server>
mcporter call <server.tool> --args '{"key": "value"}'
```

## Shared Memory Systems

| System | Location | Purpose |
|--------|----------|---------|
| facts.jsonl | /mnt/ssd/aletheia/shared/memory/ | Atomic facts (all agents write) |
| mcp-memory.json | /mnt/ssd/aletheia/shared/memory/ | Knowledge graph |
| Letta | localhost:8283 | Queryable memory |

---
*Part of unified agent ecosystem*
