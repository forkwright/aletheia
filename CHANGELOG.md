# Changelog

## [2026-02-17] - Web UI + streaming + resilience

### Added
- **Web UI** — Svelte 5 chat interface served at `/ui` from `ui/dist/`
  - Streaming responses via SSE (`POST /api/sessions/stream`)
  - Real-time event push (`GET /api/events`)
  - Per-agent conversation management, session switching
  - Markdown rendering, syntax highlighting (tree-shaken hljs), emoji support
  - Mobile-responsive with swipe sidebar
  - Dark theme, context proximity indicator
- **Streaming API** — `completeStreaming()` on AnthropicProvider + ProviderRouter
  - `StreamingEvent` types: text_delta, tool_use_start/delta/end, message_complete
  - `executeTurnStreaming()` mirrors full `executeTurn()` with incremental yields
- **Event bus → SSE bridge** — turn, tool, session events broadcast to WebUI clients
- **Cost endpoints** — `GET /api/costs/summary`, `GET /api/costs/session/:id`
- **Agent identity endpoint** — `GET /api/agents/:id/identity` (name + emoji from IDENTITY.md)
- **Session resilience** — `buildMessages()` detects orphaned `tool_use` blocks and injects synthetic `tool_result` responses. Prevents 400 errors after mid-turn service restarts.

### Fixed
- **History API** — excluded distilled messages by default (`?includeDistilled=true` to opt in). Previously LIMIT 100 could return only old superseded messages.
- **Emoji parsing** — identity endpoint extracts only Unicode emoji chars, strips trailing descriptions
- **Concurrent agent turns** — `MAX_CONCURRENT_TURNS` 3→6 (all agents share one Signal account)

### Changed
- Sidebar: removed "Agents" section header
- JS bundle: 1,031KB → 199KB (81% reduction via hljs tree-shaking)
- Runtime bundle: 293KB → 354KB
- Static file serving with SPA fallback, immutable caching for hashed assets

---

## [2026-02-17] - Competitive features (Phases 0-5)

### Added
- **MCP security** — auth bypass fix, crypto session IDs, rate limiting, scope enforcement, CORS
- **Temperature routing** — `selectTemperature()` based on message content classification
- **Memory intelligence** — `/graph/analyze` (PageRank, Louvain), `/search_enhanced` (query rewriting)
- **Self-observation** — check_calibration, what_do_i_know, recent_corrections tools
- **Mid-session context injection** — every 8 turns
- **Cross-agent blackboard** — SQLite migration v6, TTL-based expiry
- **Meta-tools** — context_check, status_report
- **Whisper transcription** — wraps whisper.cpp for voice messages
- **Browser automation** — LLM-driven browsing via run_task.py

### Added (competitive patterns)
- **GOALS.md** in bootstrap (priority 4.5, semi-static)
- **Git-tracked workspaces** — auto-commit on write/edit
- **Event bus** — 14 typed events, singleton
- **Interaction signals** — heuristic classification, stored in `interaction_signals` table
- **Dynamic tool loading** — essential vs available categories, 5-turn expiry
- **Similarity pruning** — Jaccard word overlap in distillation pipeline
- **Foresight signals** — temporal nodes in Neo4j
- **Autonomous links** — Haiku generates relationship descriptions between memories
- **Skill learning** — extracts SKILL.md from 3+ tool call trajectories

### Changed
- Tools: 22 → 28
- Tests: 705 → 772 (79 test files)
- Build: 177KB → 293KB

---

## [2026-02-16] - Foundation quality (Phase 0)

### Changed
- **Token counter** — added SAFETY_MARGIN (1.15), `estimateTokensSafe()`, `estimateToolDefTokens()` with per-tool overhead accounting
- **Bootstrap assembly** — SHA-256 content hashing (per-file + composite), dropped-file tracking, degraded-mode service injection, section-aware truncation preserving markdown headers
- **Extraction prompt** — domain-aware rewrite with anti-temporal rules, quality filters, contradiction detection, [UNCERTAIN] prefix for uncertain facts
- **Summarization** — agent-context-aware prompts via nousId, anti-photocopying instruction for repeated distillation
- **Distillation pipeline** — concurrent distillation guard, memory flush with 3x exponential backoff retry, compression ratio check (re-summarizes if > 50%), tool result sanitization (8KB cap), distillation counter with archive warning at 3+, `[Distillation #N]` markers
- **Multi-stage summarization** — large conversations split by token share, chunk summaries merged; adapted from OpenClaw's `summarizeInStages` pattern
- **DB migration v4** — `last_input_tokens`, `bootstrap_hash`, `distillation_count` columns on sessions
- **Distillation trigger** — uses actual API-reported `usage.inputTokens` instead of heuristic estimate
- Build size: 177KB → 190KB (+13KB for chunked summarization module)

---

## [2026-02-16] - Capability rebuild (Phases 1-5)

### Added
- **Signal command registry** — 10 built-in `!` commands: ping, help, status, sessions, reset, agent, skills, approve, deny, contacts
  - Commands intercepted before agent turn pipeline (no API call wasted)
  - `CommandRegistry` class with register/match/listAll pattern
- **Link pre-processing** — auto-fetches up to 3 URLs in messages, appends title + content previews
  - SSRF guard blocks private IP ranges (reuses `ssrf-guard.ts`)
- **Media understanding** — image attachments converted to Anthropic vision content blocks
  - `client.getAttachment()` now actually called (existed but was never wired)
  - Supports jpeg, png, gif, webp up to `mediaMaxMb` config limit
  - `InboundMessage.media: MediaAttachment[]` replaces unused `mediaUrls: string[]`
- **CLI admin commands** — `aletheia status`, `send`, `sessions`, `cron list`, `cron trigger`
- **Admin API endpoints** — `/api/agents`, `/api/agents/:id`, `/api/cron`, `/api/cron/:id/trigger`, `/api/sessions/:id/archive`, `/api/sessions/:id/distill`, `/api/skills`
- **Contact pairing system** — challenge-code auth flow for unknown Signal senders
  - DB migration v3: `contact_requests` + `approved_contacts` tables
  - `dmPolicy: "pairing"` now fully implemented (was stub since clean-room)
  - Admin commands: `!approve <code>`, `!deny <code>`, `!contacts`
  - API: `GET /api/contacts/pending`, `POST /api/contacts/:code/approve|deny`
- **Skills directory** — loads `SKILL.md` files from `shared/skills/` subdirectories
  - Injected into bootstrap system prompt (semi-static cache group for caching)
  - `!skills` command + `GET /api/skills` API endpoint
- **`triggerDistillation()`** method on NousManager for admin-triggered distillation
- **`findSessionsByKey()`** method on SessionStore for sender-scoped session queries

### Changed
- Media passed through to plugin `TurnContext` for hooks
- `assembleBootstrap()` accepts optional `skillsSection` for semi-static injection
- Build size: 147KB → 177KB (+30KB for all 5 phases)
- 1,141 lines added across 13 files (3 new, 10 modified)

---

## [2026-02-15] - QA remediation + memory quality

### Fixed
- **Distillation amnesia** (CRITICAL) — `isDistilled: true` on summary made it invisible to future turns
  - Summary now `isDistilled: false`, token counts recalculated in transaction
- **Tool results excluded from extraction/summarization** — now included
- **Tool definition tokens** not subtracted from history budget — now subtracted

### Added
- Watchdog service health alerts (qdrant, neo4j, mem0-sidecar, ollama)
- Cron model override (`InboundMessage.model` field)
- Concurrent agent turns (MAX_CONCURRENT_TURNS = 3, per-session mutex)
- Graceful shutdown with 10s drain period

### Changed
- Heartbeat uses Haiku (~95% token savings)
- Mem0 custom extraction prompt (domain-aware, anti-temporal)
- Neo4j: 4,688 → 28 relationship types, 801 → 735 nodes
- Voyage-3-large embeddings (1024 dims, replaces Ollama)
- Cross-agent dedup: 0.92 cosine threshold in sidecar
- OpenClaw purge: .openclaw → .aletheia, git-filter-repo strip (13MB → 7.7MB)

---

## [2026-02-14] - Clean-room runtime

### Added
- Clean-room rewrite — removed all OpenClaw/PI dependencies (789k lines, 47 packages)
- Stack: Hono (gateway), better-sqlite3 (sessions), @anthropic-ai/sdk, Zod (config), Commander (CLI)
- 6 nous, 13 tools, 1 plugin (aletheia-memory)

---

## [2026-02-13] - Dual-layer memory system

### Added
- **Mem0 integration** — automatic fact extraction from every conversation via Claude Haiku 4.5
  - Qdrant vector store for semantic memory search
  - Neo4j graph store for entity relationships
  - FastAPI sidecar service (`aletheia-memory.service`, port 8230)
  - Cross-agent shared memory (user scope) + agent-specific domain memory
- **Memory plugin** — lifecycle hooks for automatic extraction and recall
  - `before_agent_start`: searches Mem0 for relevant memories, injects into context
  - `agent_end`: sends conversation transcript to Mem0 for extraction
  - `after_compaction`: extracts session summaries into long-term memory
- **Federated memory search** — `memory_search` tool queries sqlite-vec + Mem0 in parallel, merges and deduplicates results
- **Compaction context passthrough** — `after_compaction` hooks now receive agentId, sessionKey, workspaceDir

### Changed
- Memory system section in README updated for dual-layer architecture
- RESCUE.md updated with new recovery steps and dependencies
- Config path references updated from `.openclaw/openclaw.json` to `.aletheia/aletheia.json`
- All 7 agent SOUL.md files updated with Memory section documenting automatic extraction
- Syn's MEMORY.md trimmed from 32K to 2K chars — facts offloaded to Mem0
- Watchdog: Letta check replaced with mem0-sidecar, qdrant, neo4j, ollama checks
- Sidecar routes refactored: `asyncio.to_thread` for non-blocking Mem0 operations
- `aletheia-graph` rewritten from FalkorDB (docker exec redis-cli) to Neo4j (bolt driver)
- `assemble-context` domain knowledge queries now use Neo4j via `aletheia-graph`
- `checkpoint` health check: FalkorDB ping replaced with Neo4j HTTP check
- `tools.yaml`: knowledge_graph db reference updated from falkordb to neo4j, added mem0 section
- `mem0_search` tool registered via memory plugin — agents can explicitly search long-term memories

### Removed
- FalkorDB container retired (data migrated to Neo4j, ~39MB RAM freed)
- FalkorDB-only scripts archived: `graph-genesis`, `graph-rewrite`, `graph-maintain`
- FalkorDB confidence reinforcement in `assemble-context` (handled natively by Neo4j)
- Old npm global openclaw install (`/usr/bin/openclaw`, `/usr/lib/node_modules/openclaw/`)
- `clawdbot.service.disabled` legacy systemd file

### Infrastructure
- Docker: Qdrant v1.16.2, Neo4j 2025-community
- Python: Mem0 sidecar with Ollama embeddings (mxbai-embed-large, 1024 dims)
- Systemd: `aletheia-memory.service` (uvicorn, port 8230)

### Data Migration
- facts.jsonl: 377/384 facts imported to Mem0
- mcp-memory.json: 24/24 entities imported to Mem0
- FalkorDB: 276 edges migrated directly to Neo4j (3 graphs: aletheia, knowledge, temporal_events)
- Session JSONL transcripts: blocked by Anthropic API credit exhaustion (~51 chunks of akron imported before credits ran out; remaining 7+ agents pending credits)

---

## [2026-02-08] - Session routing fix

### Fixed
- **Group message routing** — Syn's heartbeat was triggering `sessions_send` to stale `agent:main` group sessions, causing all agents to respond as Syn
  - Deleted stale `agent:main:signal:group:*` sessions
  - Added no-sessions_send rule to Syn's HEARTBEAT.md
  - Fixed Arbor model ID from invalid `claude-sonnet-4-5-20250514` to `claude-sonnet-4-5-20250929`

---

## [2026-02-13] - Context overflow fix

### Fixed
- **Context overflow on all agents** — POSIX ACL entries on `shared/bin/*` denied `syn` execute permission despite `rwx` POSIX permissions
  - `assemble-context` and `distill` both failed with EACCES
  - Compaction pipeline completely broken, sessions grew unbounded
  - Fix: `setfacl -m u:syn:rwx /mnt/ssd/aletheia/shared/bin/*`

---

## [2026-02-05] - Initial fork

### Added
- Forked from OpenClaw v2026.2.1 as Aletheia
- 7-agent multi-nous architecture
- Signal messaging via signal-cli
- sqlite-vec memory search
- Structured distillation (pre-compaction fact extraction)
- Context assembly pipeline
- Adaptive awareness (prosoche)
- Langfuse observability
