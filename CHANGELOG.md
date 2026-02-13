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

### Removed
- FalkorDB dependency (data rescued and migrated to Neo4j)
- Old npm global openclaw install (`/usr/bin/openclaw`, `/usr/lib/node_modules/openclaw/`)
- `clawdbot.service.disabled` legacy systemd file

### Infrastructure
- Docker: Qdrant v1.16.2, Neo4j 2025-community
- Python: Mem0 sidecar with Ollama embeddings (mxbai-embed-large, 1024 dims)
- Systemd: `aletheia-memory.service` (uvicorn, port 8230)

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
