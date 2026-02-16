# Changelog

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
