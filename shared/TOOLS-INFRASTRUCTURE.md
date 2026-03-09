# Shared Tools

All agents have access to shared scripts at `$ALETHEIA_ROOT/shared/bin/`:

| Tool | Purpose |
|------|---------|
| aletheia-backup | Timestamped backup of all Aletheia state (config, sessions, workspaces) |
| credential-refresh | Auto-refresh Anthropic OAuth tokens before expiry |
| gcal | Google Calendar event query |
| pplx | Perplexity pro-search |
| scholar | Multi-source academic search (OpenAlex + arXiv + Semantic Scholar) |
| start.sh | Template startup script for ~/.aletheia/start.sh |
| transcribe | Audio transcription via Whisper |
| wiki | Wikipedia lookup for concept verification |

## Memory Systems

| System | Location | Purpose |
|--------|----------|---------|
| KnowledgeStore | CozoDB (embedded) | Long-term extracted memories, entity relationships, vector search |
| Blackboard | sessions.db | Cross-agent shared state (TTL-based, SQLite) |

## Built-in Runtime Tools

Essential (always available): read, write, edit, ls, find, grep, exec, memory_search, sessions_send, sessions_spawn, enable_tool, deliberate

Available (on-demand via enable_tool): research, transcribe, browser_use, blackboard, check_calibration, what_do_i_know, recent_corrections, context_check, status_report, gateway + others
