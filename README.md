# Aletheia

*Multi-agent AI system infrastructure coordinating specialized agents through a custom web UI & Signal messaging.*

Self-hosted, privacy-first. Runs on most any hardware as a systemd service.

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
         | Syn |  | ...  | | ...  | | ...  |   N agents, each with:
         +-----+  +------+ +------+ +------+   - SOUL.md (character)
            |         |         |        |       - AGENTS.md (operations)
            v         v         v        v       - MEMORY.md (continuity)
         Claude     Claude    Claude   Claude
```

**Runtime**: Node.js >=22.12, TypeScript compiled with tsdown (~354KB bundle), Hono gateway on port 18789. Web UI at `/ui` (Svelte 5).

**Communication**: Fully featured web chat with file upload. Signal messenger via signal-cli. 10 built-in `!` commands. Link pre-processing with SSRF guard. Image vision via Anthropic content blocks. Contact pairing with challenge codes.

**Models**: Currently only piped for Anthropic oauth or API. Complexity-based routing with temperature selection. Provider failover across Anthropic, OpenRouter, OpenAI, and Azure being built.

**Memory**: Dual-layer — Mem0 (AI extraction via Claude Haiku, Qdrant vectors, Neo4j graph) for automatic cross-agent long-term memory + sqlite-vec for fast local per-agent vector search. Cross-agent blackboard (SQLite, TTL-based). Self-observation tools (calibration, correction tracking).

**Observability**: Self-hosted Langfuse (port 3100) for session traces and metrics.

---

## Directory Structure

```
aletheia/
├── nous/                   Agent workspaces
│   ├── _example/           Template workspace (start here)
│   └── {agent}/
│       ├── SOUL.md             Character definition (prose)
│       ├── USER.md             Human operator context
│       ├── AGENTS.md           Operations guide
│       ├── MEMORY.md           Curated long-term memory
│       ├── GOALS.md            Goal hierarchy
│       ├── IDENTITY.md         Name, emoji, metadata
│       ├── PROSOCHE.md         Directed awareness config
│       ├── TOOLS.md            Tool reference (generated)
│       ├── CONTEXT.md          Session-scoped dynamic context
│       └── memory/             Daily logs, session state
│
├── shared/                 Common infrastructure
│   ├── bin/                Shared scripts (on PATH for all agents)
│   ├── templates/          Shared sections + per-agent YAML → compiled files
│   ├── config/             aletheia.env, tools.yaml, provider-failover.json
│   ├── schemas/            JSON schemas (agent-contract, task-contract)
│   └── skills/             Shared agent skills
│
├── infrastructure/
│   ├── runtime/            Gateway (TypeScript/tsdown)
│   │   ├── src/                TypeScript source
│   │   │   ├── taxis/              Config loading + validation (Zod)
│   │   │   ├── mneme/              Session store (better-sqlite3)
│   │   │   ├── hermeneus/          Anthropic SDK + provider router
│   │   │   ├── organon/            Tool registry + 28 built-in tools + skills
│   │   │   ├── semeion/            Signal client, listener, commands, preprocessing
│   │   │   ├── pylon/              Hono HTTP gateway, MCP, Web UI
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
├── ui/                     Web UI (Svelte 5 + TypeScript)
│   ├── src/                    Components, stores, lib
│   └── dist/                   Build output (served at /ui)
│
├── config/                 Example configuration
│   └── aletheia.example.json
├── ALETHEIA.md             System manifesto
├── RESCUE.md               Recovery guide
└── docker-compose.yml      signal-cli container
```

---

## Agents

Each agent has a dedicated workspace under `nous/` with character (`SOUL.md`), operations (`AGENTS.md`), and long-term memory (`MEMORY.md`). See `nous/_example/` for a complete template.

Agents are defined in the gateway config (`aletheia.json`). Each agent has:
- A unique ID and model configuration with fallback chains
- Signal bindings (DM routing, group chat assignment)
- Workspace directory with bootstrap files
- Optional cron jobs and skills

---

## Memory System

### Mem0 Long-Term Memory (Primary)

AI-powered memory extraction and retrieval. Every conversation is automatically processed by Claude Haiku to extract facts, entity relationships, and preferences. Stored in Qdrant (vector search) and Neo4j (graph relationships).

- **Automatic extraction**: `agent_end` hook sends conversation transcripts to Mem0 for fact extraction
- **Pre-session recall**: `before_agent_start` hook searches Mem0 for relevant memories and injects them into context
- **Cross-agent**: Shared `user_id` scope allows any agent to recall facts learned by other agents
- **Agent-scoped**: Domain-specific memories scoped to individual agents via `agent_id`
- **Graph search**: Entity relationship traversal via Neo4j

Services: Mem0 sidecar (:8230), Qdrant (:6333), Neo4j (:7474/:7687)

### Local Memory (sqlite-vec)

Built into the gateway runtime. Per-agent vector search over workspace files (MEMORY.md, daily logs). Federated with Mem0 — the `memory_search` tool queries both backends in parallel and merges results.

### Context Assembly

At session start, bootstrap assembles: agent workspace files + recalled memories + task state. During context pressure, the distillation pipeline extracts structured insights before compression. Post-session, the memory plugin extracts summaries into Mem0.

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
| GET | `/api/agents/:id/identity` | Agent name + emoji from IDENTITY.md |
| GET | `/api/sessions` | Session list (optional `?nousId=`) |
| GET | `/api/sessions/:id/history` | Message history (excludes distilled by default) |
| POST | `/api/sessions/send` | Send message to agent |
| POST | `/api/sessions/stream` | Streaming message (SSE: turn_start, text_delta, tool events, turn_complete) |
| POST | `/api/sessions/:id/archive` | Archive session |
| POST | `/api/sessions/:id/distill` | Trigger distillation |
| GET | `/api/events` | SSE event stream (turn, tool, session events) |
| GET | `/api/costs/summary` | Token usage and cost summary |
| GET | `/api/costs/session/:id` | Per-session cost breakdown |
| GET | `/api/cron` | Cron job list |
| POST | `/api/cron/:id/trigger` | Manual cron trigger |
| GET | `/api/skills` | Skills directory |
| GET | `/api/contacts/pending` | Pending contact requests |
| POST | `/api/contacts/:code/approve` | Approve contact |
| POST | `/api/contacts/:code/deny` | Deny contact |
| GET | `/api/config` | Config summary |

### Web UI

Svelte 5 chat interface at `/ui`. Streaming responses via SSE, real-time event push, per-agent conversation management.

```bash
cd ui && npm install && npm run build    # Outputs to ui/dist/
```

The gateway serves `ui/dist/` as static files with SPA fallback. If no build exists, falls back to a minimal status dashboard.

### Shared Scripts (`shared/bin/`)

| Command | Purpose |
|---------|---------|
| `pplx "query"` | Perplexity pro-search |
| `scholar "topic"` | Academic search (OpenAlex + arXiv + Semantic Scholar) |
| `browse "task"` | LLM-driven web automation |
| `ingest-doc file.pdf` | PDF/DOCX extraction to markdown |
| `aletheia-graph query "..."` | Knowledge graph CLI (Neo4j) |
| `nous-health` | Monitor nous ecosystem health |

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
npx tsdown            # Single entry point → dist/entry.js
```

### Running

```bash
# Start memory infrastructure
cd infrastructure/memory && docker compose up -d

# Start the gateway
aletheia gateway
# or: node infrastructure/runtime/aletheia.mjs gateway
```

### Systemd Service

The gateway runs as a systemd service under a dedicated service account.

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

Copy `.env.example` to `shared/config/aletheia.env` and fill in values. Must be systemd `EnvironmentFile` compatible (no `export`, no variable refs).

---

## Configuration

| File | Purpose |
|------|---------|
| `~/.aletheia/aletheia.json` | Gateway config (agents, bindings, routing, sessions, cron, watchdog) |
| `~/.aletheia/credentials/` | API credentials |
| `config/aletheia.example.json` | Example gateway config |
| `infrastructure/memory/sidecar/aletheia_memory/config.py` | Mem0 backend configuration |
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
src/mneme/            Session store (better-sqlite3, 6 migrations)
src/hermeneus/        Anthropic SDK, provider router, token counting
src/organon/          Tool registry, 28 built-in tools, skills directory
src/semeion/          Signal client, SSE listener, commands, link preprocessing
src/pylon/            Hono HTTP gateway, MCP, Web UI
src/prostheke/        Plugin system (lifecycle hooks)
src/nous/             Bootstrap assembly, agent turn execution (NousManager)
src/distillation/     Context summarization pipeline
src/daemon/           Cron scheduler, watchdog health probes
src/koina/            Shared utilities (logger, token counter)
```

### Template Compilation

Agent workspace files (`AGENTS.md`, `PROSOCHE.md`, `TOOLS.md`) can be compiled from shared templates + per-agent YAML configs:

```bash
compile-context         # Regenerate all workspace files
compile-context syn     # Regenerate for specific agent
```

Source: `shared/templates/sections/*.md` + `shared/templates/agents/*.yaml`

---

## Recovery

See `RESCUE.md` for full restoration from scratch.

---

## License

AGPL-3.0 (runtime) + Apache-2.0 (SDK/client)

*Built by forkwright, 2026*
