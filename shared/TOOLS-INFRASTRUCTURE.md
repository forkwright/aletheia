# Shared Tools

All agents have access to shared tools at `$ALETHEIA_ROOT/shared/bin/`:

| Tool | Purpose |
|------|---------|
| pplx | Perplexity search |
| scholar | Multi-source academic search |
| browse | LLM-driven web automation |
| ingest-doc | PDF/document extraction to markdown |
| aletheia-graph | Knowledge graph CLI (Neo4j) |
| nous-health | Monitor agent ecosystem health |

## Memory Systems

| System | Location | Purpose |
|--------|----------|---------|
| sqlite-vec | Per-agent workspace | Fast local search over MEMORY.md, daily logs, workspace files |
| Mem0 | localhost:8230 | Long-term extracted memories (cross-agent, cross-session) |
| Neo4j | localhost:7687 | Entity relationship graph |

---
*Part of Aletheia distributed cognition system*
