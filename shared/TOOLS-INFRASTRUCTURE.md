# Shared Tools

All agents have access to shared scripts and built-in commands:

| Tool | Purpose |
|------|---------|
| `aletheia backup` | Whole-instance backup (config, sessions, workspaces) |
| credential-refresh | Auto-refresh Anthropic OAuth tokens before expiry |
| gcal | Google Calendar event query |
| pplx | Perplexity pro-search |
| scholar | Multi-source academic search (OpenAlex + arXiv + Semantic Scholar) |
| start.sh | Instance startup script; sources `$ALETHEIA_ROOT/config/env` and launches `$ALETHEIA_BIN` (instance root defaults to `$HOME/aletheia/instance`) |
| transcribe | Audio transcription via Whisper |
| wiki | Wikipedia lookup for concept verification |

## Memory Systems

| System | Location | Purpose |
|--------|----------|---------|
| KnowledgeStore | mneme (embedded Datalog engine) | Long-term extracted memories, entity relationships, vector search |
| Blackboard | instance data store | Cross-agent shared state |

## Built-in runtime tools

Essential (always available): read, write, edit, ls, find, grep, exec, memory_search, sessions_send, sessions_spawn, enable_tool, deliberate

Available (on-demand via enable_tool): research, transcribe, browser_use, blackboard, check_calibration, what_do_i_know, recent_corrections, context_check, status_report, gateway + others
