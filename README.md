# Aletheia

*Multi-agent AI system coordinating 6 specialized agents through Signal messaging.*

Self-hosted, privacy-first. Runs on a home server as a systemd service.

---

## Architecture

```
                    Signal Messenger
                         |
                    signal-cli (JSON-RPC, :8080)
                         |
                  +--------------+
                  |   Aletheia   |     Node.js gateway (TypeScript/tsdown)
                  |   Gateway    |     Session management, tool execution,
                  |   (:18789)   |     message routing, context assembly
                  +--------------+
                   /    |    |   \
              Bindings (per-agent group routing)
                /       |    |      \
         +-----+  +------+ +------+ +------+
         | Syn |  | Syl  | |Arbor | | ...  |   6 agents, each with:
         +-----+  +------+ +------+ +------+   - SOUL.md (character)
            |         |         |        |       - AGENTS.md (operations)
            v         v         v        v       - MEMORY.md (continuity)
         Claude     Claude    Claude   Claude
        Opus 4.6  Opus 4.6  Opus 4.6  Opus 4.6
```

**Runtime**: Node.js >=22.12, TypeScript compiled with tsdown (~177KB bundle), Hono gateway on port 18789

**Communication**: Signal messenger via signal-cli (JSON-RPC mode on port 8080). 10 built-in `!` commands. Link pre-processing with SSRF guard. Image vision via Anthropic content blocks. Contact pairing with challenge codes.

**Models**: Claude Opus 4.6 (primary), Claude Sonnet 4 (fallback), Gemini Flash (fallback). Provider failover across Anthropic, OpenRouter, OpenAI, and Azure.

**Memory**: Dual-layer — Mem0 (AI extraction via Claude Haiku, Qdrant vectors, Neo4j graph) for automatic cross-agent long-term memory + sqlite-vec for fast local per-agent vector search. JSONL fact store with confidence scoring.

**Observability**: Self-hosted Langfuse (port 3100) for session traces and metrics.

---

## Directory Structure

```
/mnt/ssd/aletheia/
├── nous/                   Agent workspaces (6 agents)
│   └── {agent}/
│       ├── SOUL.md             Character definition (prose)
│       ├── AGENTS.md           Operations (compiled from templates)
│       ├── MEMORY.md           Curated long-term memory
│       ├── PROSOCHE.md         Directed awareness config
│       ├── TOOLS.md            Tool reference (generated)
│       ├── memory/             Daily logs, session state
│       └── docs/               Agent-specific documentation
│
├── shared/                 Common infrastructure
│   ├── bin/                ~70 scripts (on PATH for all agents)
│   ├── templates/          Shared sections + per-agent YAML → compiled files
│   ├── config/             aletheia.env, tools.yaml, provider-failover.json
│   ├── contracts/          Agent capability contracts (JSON)
│   ├── memory/             facts.jsonl, knowledge graph data
│   ├── schemas/            JSON schemas (agent-contract, task-contract)
│   ├── skills/             Shared agent skills
│   ├── status/             Service status tracking
│   └── checkpoints/        System state snapshots
│
├── infrastructure/
│   ├── runtime/            Clean-room gateway (TypeScript/tsdown)
│   │   ├── src/                TypeScript source (~177KB compiled)
│   │   │   ├── taxis/              Config loading + validation (Zod)
│   │   │   ├── mneme/              Session store (better-sqlite3)
│   │   │   ├── hermeneus/          Anthropic SDK + provider router
│   │   │   ├── organon/            Tool registry + built-in tools + skills
│   │   │   ├── semeion/            Signal client, listener, commands, preprocessing
│   │   │   ├── pylon/              Hono HTTP gateway (17 endpoints)
│   │   │   ├── prostheke/          Plugin system (lifecycle hooks)
│   │   │   ├── nous/               Agent bootstrap + turn execution
│   │   │   ├── distillation/       Context summarization pipeline
│   │   │   ├── daemon/             Cron scheduler + watchdog
│   │   │   └── koina/              Shared utilities (logger, token counter)
│   │   ├── dist/               Compiled output
│   │   └── aletheia.mjs        Entry point
│   ├── memory/             Mem0 sidecar + docker-compose (Qdrant, Neo4j)
│   │   ├── sidecar/            FastAPI Mem0 wrapper (Python/uvicorn)
│   │   ├── aletheia-memory/    Memory plugin (lifecycle hooks + mem0_search tool)
│   │   └── docker-compose.yml  Qdrant + Neo4j containers
│   ├── langfuse/           Self-hosted observability (Docker)
│   └── prosoche/           Adaptive attention daemon
│
├── theke/                  Obsidian vault (human-facing, gitignored)
├── projects/               Project backing store (gitignored)
├── archive/                Historical files (gitignored)
├── ALETHEIA.md             System manifesto
├── RESCUE.md               Full restoration guide
└── docker-compose.yml      Legacy signal-cli container
```

---

## Agents

Each agent has a dedicated workspace under `nous/` with character (`SOUL.md`), operations (`AGENTS.md`), and long-term memory (`MEMORY.md`).

| Agent | Greek | Domain | Binding |
|-------|-------|--------|---------|
| **Syn** | synnous -- thinking together | Orchestrator, primary | Signal DM (default) |
| **Eiron** | eiron -- discriminator | MBA coursework, academic | Signal DM (routed) |
| **Demiurge** | demiourgos -- craftsman | Creative, craft, leatherwork | Signal DM (routed) |
| **Syl** | syllepsis -- grasping together | General assistant, family, home | Family group chat |
| **Arbor** | rooted | Work (Summus healthcare) | Arbor group chat |
| **Akron** | akron -- summit | Vehicle, preparedness, technical | Signal DM (routed) |

**Routing**: Syn is the default agent for direct messages. Other agents are routed via Signal group bindings or explicit routing rules. Agent contracts in `shared/contracts/` define capabilities, interfaces, and session keys.

---

## Memory System

### Mem0 Long-Term Memory (Primary)

AI-powered memory extraction and retrieval. Every conversation is automatically processed by Claude Haiku to extract facts, entity relationships, and preferences. Stored in Qdrant (vector search) and Neo4j (graph relationships).

- **Automatic extraction**: `agent_end` hook sends conversation transcripts to Mem0 for fact extraction
- **Pre-session recall**: `before_agent_start` hook searches Mem0 for relevant memories and injects them into context
- **Cross-agent**: Shared `user_id` scope allows any agent to recall facts learned by other agents
- **Agent-scoped**: Domain-specific memories scoped to individual agents via `agent_id`
- **Graph search**: Entity relationship traversal via Neo4j (e.g., "what do I know about X?")

Services: Mem0 sidecar (:8230), Qdrant (:6333), Neo4j (:7474/:7687)

### Local Memory (sqlite-vec)

Built into the gateway runtime. Per-agent vector search over workspace files (MEMORY.md, daily logs). Federated with Mem0 — the `memory_search` tool queries both backends in parallel and merges results.

### Fact Store (JSONL)

Structured facts with confidence scores at `shared/memory/facts.jsonl`. Managed via `facts` CLI. Imported into Mem0 for unified search.

### Context Assembly

At session start, `assemble-context` compiles: agent workspace files + recent facts + task state. Pre-compaction, `distill` extracts structured insights before context compression. Post-compaction, the memory plugin extracts session summaries into Mem0.

---

## Interfaces

### Signal Commands

| Command | Purpose |
|---------|---------|
| `!status` | System status (uptime, services, per-nous metrics) |
| `!help` | List all available commands |
| `!ping` | Liveness check |
| `!sessions` | List active sessions for this sender |
| `!reset` | Archive current session, start fresh |
| `!agent` | Show which agent handles this conversation |
| `!skills` | List available skills |
| `!approve <code>` | Approve pending contact request (admin) |
| `!deny <code>` | Deny pending contact request (admin) |
| `!contacts` | List pending contact requests (admin) |

### CLI

| Command | Purpose |
|---------|---------|
| `aletheia gateway` | Start the runtime (default) |
| `aletheia status` | System status via `/api/metrics` |
| `aletheia send -a <agent> -m "..."` | Send message to agent |
| `aletheia sessions [-a <agent>]` | List sessions |
| `aletheia cron list` | List cron jobs |
| `aletheia cron trigger <id>` | Trigger a cron job |
| `aletheia doctor` | Connectivity checks |

### API Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/health` | Health check (no auth) |
| GET | `/api/status` | Agent list + timestamp |
| GET | `/api/metrics` | Full metrics (per-nous, tokens, cache, cron, services) |
| GET | `/api/agents` | All agents with model info |
| GET | `/api/agents/:id` | Single agent + recent sessions + usage |
| GET | `/api/sessions` | Session list (optional `?nousId=`) |
| GET | `/api/sessions/:id/history` | Message history |
| POST | `/api/sessions/send` | Send message to agent |
| POST | `/api/sessions/:id/archive` | Archive session |
| POST | `/api/sessions/:id/distill` | Trigger distillation |
| GET | `/api/cron` | Cron job list |
| POST | `/api/cron/:id/trigger` | Manual cron trigger |
| GET | `/api/skills` | Skills directory |
| GET | `/api/contacts/pending` | Pending contact requests |
| POST | `/api/contacts/:code/approve` | Approve contact |
| POST | `/api/contacts/:code/deny` | Deny contact |
| GET | `/api/config` | Config summary |

### Shared Scripts (`shared/bin/`)

| Command | Purpose |
|---------|---------|
| `pplx "query"` | Perplexity pro-search |
| `scholar "topic"` | Academic search (OpenAlex + arXiv + Semantic Scholar) |
| `browse "url"` | LLM-driven web automation |
| `ingest-doc file.pdf` | PDF/DOCX extraction to markdown |
| `compile-context` | Regenerate workspace files from templates |
| `aletheia-graph query "..."` | Knowledge graph CLI (Neo4j) |

---

## Deployment

### Prerequisites

- Node.js >=22.12
- signal-cli (native install or container)
- Docker (for Qdrant, Neo4j, Langfuse)

### Building

```bash
cd infrastructure/runtime
npm install
npx tsdown            # Single entry point → dist/entry.js (~177KB)
```

### Deploying

```bash
# On server as syn user:
cd /mnt/ssd/aletheia
git pull origin main
cd infrastructure/runtime && npx tsdown
sudo systemctl restart aletheia
```

### Systemd Service

The gateway runs as `aletheia.service` under the `syn` service account.

```bash
sudo systemctl status aletheia
sudo systemctl restart aletheia
journalctl -u aletheia -f
```

### Services

| Service | Port | Notes |
|---------|------|-------|
| aletheia | 18789 | Gateway + Signal listener |
| signal-cli | 8080 | JSON-RPC (localhost only) |
| aletheia-memory | 8230 | Mem0 sidecar (FastAPI/uvicorn) |
| qdrant | 6333 | Vector store (127.0.0.1) |
| neo4j | 7474/7687 | Graph store (127.0.0.1) |
| langfuse | 3100 | Observability |

### Environment

`aletheia.env` in shared/config/ — must be systemd `EnvironmentFile` compatible (no `export`, no variable refs).

Key env vars: `ANTHROPIC_API_KEY`, `BRAVE_API_KEY`, `ALETHEIA_ROOT`, `CHROMIUM_PATH`.

---

## Configuration

| File | Purpose |
|------|---------|
| `/home/syn/.aletheia/aletheia.json` | Gateway config (agents, bindings, routing, sessions, cron, watchdog) |
| `/home/syn/.aletheia/credentials/anthropic.json` | Anthropic OAuth token |
| `infrastructure/memory/sidecar/aletheia_memory/config.py` | Mem0 backend configuration |
| `shared/contracts/*.json` | Per-agent capability contracts |
| `shared/templates/` | Template sections + per-agent YAML for compiled workspace files |
| `shared/skills/*/SKILL.md` | Agent skills (loaded into bootstrap) |

**Config reload**: Changes require `systemctl restart aletheia`.

---

## Development

### Module Architecture

```
src/entry.ts          CLI entry point (Commander)
src/aletheia.ts       Main orchestration — wires all modules
src/taxis/            Config loading + Zod validation
src/mneme/            Session store (better-sqlite3, 3 migrations)
src/hermeneus/        Anthropic SDK, provider router, token counting
src/organon/          Tool registry, 13 built-in tools, skills directory
src/semeion/          Signal client, SSE listener, commands, link preprocessing
src/pylon/            Hono HTTP gateway (17 endpoints)
src/prostheke/        Plugin system (lifecycle hooks)
src/nous/             Bootstrap assembly, agent turn execution (NousManager)
src/distillation/     Context summarization pipeline
src/daemon/           Cron scheduler, watchdog health probes
src/koina/            Shared utilities (logger, token counter)
```

### Template Compilation

Agent workspace files (`AGENTS.md`, `PROSOCHE.md`, `TOOLS.md`) are compiled from shared templates + per-agent YAML configs:

```bash
compile-context         # Regenerate all workspace files
compile-context syn     # Regenerate for specific agent
```

Source: `shared/templates/sections/*.md` + `shared/templates/agents/*.yaml`

---

## Recovery

See `RESCUE.md` for full restoration from scratch (requires only this repo + a server).

---

*Built by forkwright, 2026*
