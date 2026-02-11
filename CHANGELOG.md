# Changelog

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
