# Shared Tools

All agents have access to shared scripts at `$ALETHEIA_ROOT/shared/bin/`:

| Tool | Purpose |
|------|---------|
| pplx | Perplexity pro-search |
| scholar | Multi-source academic search (OpenAlex + arXiv + Semantic Scholar) |
| browse | LLM-driven web automation |
| ingest-doc | PDF/document extraction to markdown |
| aletheia-graph | Knowledge graph CLI (Neo4j) |
| nous-health | Monitor agent ecosystem health |
| compile-context | Regenerate AGENTS.md + PROSOCHE.md from templates |
| generate-tools-md | Regenerate TOOLS.md for all agents |

## Memory Systems

| System | Location | Purpose |
|--------|----------|---------|
| sqlite-vec | Per-agent workspace | Fast local search over MEMORY.md and workspace files |
| Mem0 | localhost:8230 | Long-term extracted memories (cross-agent, cross-session) |
| Neo4j | localhost:7687 | Entity relationship graph (auto-extracted) |
| Blackboard | sessions.db | Cross-agent shared state (TTL-based, SQLite) |

## Built-in Runtime Tools (28)

Essential (always available): read, write, edit, ls, find, grep, exec, mem0_search, sessions_send, sessions_spawn, enable_tool, deliberate

Available (on-demand via enable_tool): research, transcribe, browser_use, blackboard, check_calibration, what_do_i_know, recent_corrections, context_check, status_report, gateway + others
