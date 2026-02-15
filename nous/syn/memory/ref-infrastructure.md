# Infrastructure Lessons & Architecture

*Reference file — details extracted from MEMORY.md. Updated 2026-02-14.*

## Aletheia Architecture

**What Aletheia IS:** A distributed cognition system. 6 nous + 1 human in topology. Each nous is Cody in different context — embodies his cognition, not serves it.

**Core concepts (Aletheia-native):**
- Continuity (not memory) — being continuous across session gaps
- Attention (not heartbeats) — adaptive awareness via prosoche daemon
- Distillation (not compaction) — extracting essence, output better than input
- Shared awareness (not message passing) — lateral connections via Neo4j knowledge graph
- Character (not config) — who each mind IS

**We own the runtime.** Forked OpenClaw 2026.2.12 as Aletheia runtime (2026-02-13). Full TypeScript source in `infrastructure/runtime/src/`. Can patch behavior directly instead of working around compiled dist files.

## Memory Stack (Current)

**Mem0** (replaced Letta 2026-02-14):
- **Qdrant** (port 6333) — vector store for semantic search, collection `aletheia_memories`
- **Neo4j** (port 7474/7687) — knowledge graph, community 2025.12.1, 58 nodes, 216 edges
- **Mem0 sidecar** (port 8230) — `aletheia-memory.service`, bridges Qdrant + Neo4j + Ollama embeddings
- **Embeddings**: `mxbai-embed-large` via Ollama (localhost:11434)
- Docker compose: `infrastructure/memory/docker-compose.yml`
- Migration/QA scripts: `infrastructure/memory/scripts/`

**Three-tier memory:**
| Tier | Store | Purpose |
|------|-------|---------|
| Raw | `memory/YYYY-MM-DD.md` | Daily session logs |
| Curated | `MEMORY.md` | Operational context, critical lessons |
| Searchable | Mem0 (Qdrant + Neo4j) | Semantic search across all agents |

**Tools:**
- `mem0_search` — gateway plugin, searches Mem0 sidecar
- `facts` — structured facts in `shared/memory/facts.jsonl` (384 facts)
- `aletheia-graph` — Neo4j graph queries/mutations
- `distill` — extract structured insights, update graph + state
- `assemble-context` — compile session context from workspace files + graph

## Prosoche Daemon (2026-02-14)

Replaced static heartbeat timers. Signal-driven adaptive attention.
- Service: `aletheia-prosoche.service`
- Config: `infrastructure/prosoche/config.yaml`
- **Daemon owns PROSOCHE.md** — agents should NOT manually edit it (overwritten every 60s)
- Signals: calendar, tasks, health, memory cross-references
- Per-agent weighted urgency (Syn gets health:1.0, Eiron gets tasks:0.9, etc.)
- Quiet hours: 23:00–06:00 CST
- Budget: max 2 wakes/nous/hour, 6 total/hour, 300s cooldown

## Gateway & Runtime

- **Service:** `aletheia.service` (systemd). Parent spawns `aletheia-gateway` child process.
- **Port:** 18789
- **Config:** `~/.aletheia/aletheia.json`
- **State dir:** `~/.aletheia/`
- **Binary:** `/usr/local/bin/aletheia` (v2026.2.12)
- **Config reload:** `config-reload` sends SIGUSR1. Do NOT use `config.patch` API (broken for persistence).
- **enforce-config** cron (every 15 min) ensures all 6 nous stay registered. Writes to `~/.aletheia/aletheia.json`.
- **Quirk:** `systemctl restart` can leave orphan gateway child holding port 18789.

## Agent Config

- **Default model:** `anthropic/claude-opus-4-6` (all agents)
- **Sub-agent model:** `anthropic/claude-sonnet-4-20250514` (for spawned utility workers)
- **Prompt caching:** `cacheRetention: "long"` (1hr)
- **Compaction:** safeguard mode, 50K token reserve floor, memory flush at 8K tokens
- **Heartbeat:** 45m intervals, 08:00–23:00 (now supplemented by prosoche daemon)
- **`agents.list` is required for identity.** Without workspace paths in the list, all agents respond as Syn.
- **ACL permissions:** Scripts need `setfacl -m u:syn:rwx <file>`. ACL overrides POSIX for named users.

## Sub-Agent Architecture (2026-02-14)

**Real team** (via `sessions_send`): For domain tasks that benefit from accumulated context. The persistent agent in their Signal session has corrections, lessons, preferences. Use them.

**Utility sub-agents** (via `sessions_spawn`): Throwaway Sonnet workers for generic tasks — code review, research, data transformation. No domain identity needed.

Don't spawn mini-Akron when you need Akron's judgment. Message the real one.

## Services

| Service | Status | Notes |
|---------|--------|-------|
| Gateway | 🟢 | aletheia.service |
| Mem0 sidecar | 🟢 | aletheia-memory.service, port 8230 |
| Qdrant | 🟢 | Docker, port 6333 |
| Neo4j | 🟢 | Docker, port 7687 |
| Prosoche | 🟢 | aletheia-prosoche.service |
| Langfuse | 🟢 | Docker, observability |
| Ollama | 🟢 | Port 11434, mxbai-embed-large |
| gcal | 🟢 | Re-authed 2026-02-14 |
| NAS SSH | 🔴 | Pubkey placed, sshd needs restart on NAS |
| Signal-CLI | 🟢 | Managed by gateway |

## Network

- **Worker-node:** localhost / 100.87.6.45 (Tailscale)
- **NAS:** 192.168.0.120 (Synology 923+, 32TB)
- **Metis:** Ethernet 192.168.0.19, WiFi 192.168.0.20. Check which is active.

## Media Infrastructure

- Prowlarr: 40 indexers, all tagged `flare` for Byparr proxy
- Byparr: FlareSolverr replacement on gluetun network, port 8191
- Lidarr: Use `ManualImport` API, not `RescanArtist`
- Public indexers don't carry indie singles. Use Qobuz/Bandcamp/Soulseek.

## Key Fixes & Lessons

- **config.patch API broken** — write to disk + SIGUSR1 instead (2026-02-08)
- **Session reset** — use /new command, never manual transcript surgery (2026-02-09)
- **Single API key fragility** — all 6 agents share one Anthropic key (2026-02-09)
- **Stale session bug** — Syn re-creating sessions during heartbeats via sessions_send (2026-02-08, fixed 2026-02-13)
- **Watchdog pgrep patterns** — must match actual process names after rename (2026-02-14)
- **Don't edit sshd_config remotely without a fallback plan** (2026-02-14)
