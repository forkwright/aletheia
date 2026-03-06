# Shared Tools

All agents have access to shared scripts at `$ALETHEIA_ROOT/shared/bin/`:

| Tool | Purpose |
|------|---------|
| aletheia-backup | Timestamped backup of all Aletheia state (config, sessions, workspaces) |
| aletheia-export | Export an agent to a portable AgentFile |
| aletheia-graph | Knowledge graph CLI (Neo4j) |
| aletheia-setup | Install and start Aletheia systemd user services |
| aletheia-update | Self-update: pull, build, restart, health-check with auto-rollback |
| audit-tokens | Measure token consumption per bootstrap section for an agent |
| config-reload | Reload gateway config from disk without restart |
| consolidate-memory | Prune daily memory files older than retention threshold |
| credential-refresh | Auto-refresh Anthropic OAuth tokens before expiry |
| gcal | Google Calendar event query |
| nous-health | Monitor agent ecosystem health via gateway API |
| pplx | Perplexity pro-search |
| scholar | Multi-source academic search (OpenAlex + arXiv + Semantic Scholar) |
| start.sh | Template startup script for ~/.aletheia/start.sh |
| transcribe | Audio transcription via Whisper |
| wiki | Wikipedia lookup for concept verification |

## Memory Systems

| System | Location | Purpose |
|--------|----------|---------|
| sqlite-vec | Per-agent workspace | Fast local search over MEMORY.md and workspace files |
| KnowledgeStore | CozoDB (embedded) | Long-term extracted memories (cross-agent, cross-session) |
| Neo4j | localhost:7687 | Entity relationship graph (auto-extracted) |
| Blackboard | sessions.db | Cross-agent shared state (TTL-based, SQLite) |

## Built-in Runtime Tools

Essential (always available): read, write, edit, ls, find, grep, exec, memory_search, sessions_send, sessions_spawn, enable_tool, deliberate

Available (on-demand via enable_tool): research, transcribe, browser_use, blackboard, check_calibration, what_do_i_know, recent_corrections, context_check, status_report, gateway + others
