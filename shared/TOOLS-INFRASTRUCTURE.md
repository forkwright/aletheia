# Shared Tools

All nous have access to shared tools at `/mnt/ssd/aletheia/shared/bin/`:

| Tool | Purpose |
|------|---------|
| gcal | Google Calendar |
| gdrive | Google Drive |
| tw | Taskwarrior |
| pplx | Perplexity search |
| facts | Atomic fact store (shared) |
| mcporter | MCP server interface |
| memory_search | Semantic recall (sqlite-vec + Mem0) |
| aletheia-graph | Knowledge graph (Neo4j) |
| distill | Pre-compaction fact extraction |
| assemble-context | Session bootstrap context |
| compile-context | Generate CONTEXT.md from config |
| generate-context | Generate CONTEXT.md from aletheia.json |

## MCP Servers (via mcporter)

| Server | Tools | Purpose |
|--------|-------|---------|
| memory | 9 | Knowledge graph (entities, relations, observations) |
| github | 26 | GitHub API |

**Usage:**
```bash
mcporter list <server>
mcporter call <server.tool> --args '{"key": "value"}'
```

## Memory Systems

| System | Location | Purpose |
|--------|----------|---------|
| sqlite-vec | Per-nous workspace | Fast local search over MEMORY.md, daily logs, workspace files |
| Mem0 | localhost:8230 | Long-term extracted memories (cross-nous, cross-session) |
| Neo4j | localhost:7687 | Entity relationship graph |
| facts.jsonl | /mnt/ssd/aletheia/shared/memory/ | Structured atomic facts |

---
*Part of Aletheia distributed cognition system*
