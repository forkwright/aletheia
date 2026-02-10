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

**Runtime**: Node.js >=22.12, TypeScript compiled with tsdown, Express gateway on port 18789

**Communication**: Signal messenger via signal-cli (JSON-RPC mode, Docker container on port 8080). Each agent binds to specific Signal groups or DM routing patterns.

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
│   ├── runtime/            Aletheia gateway (forked OpenClaw, patched)
│   │   ├── src/                TypeScript source
│   │   ├── dist/               Compiled output
│   │   ├── extensions/signal/  Signal channel plugin
│   │   ├── skills/             Runtime skills
│   │   └── aletheia.mjs        Entry point
│   ├── memory/             Mem0 sidecar + docker-compose (Qdrant, Neo4j)
│   │   ├── sidecar/            FastAPI Mem0 wrapper (Python/uvicorn)
│   │   ├── plugin/             Aletheia memory plugin (lifecycle hooks)
│   │   └── docker-compose.yml  Qdrant + Neo4j containers
│   ├── langfuse/           Self-hosted observability (Docker)
│   └── patches/            Runtime patches (workspace, dynamic context)
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

## Tooling

### Research

| Command | Purpose |
|---------|---------|
| `pplx "query"` | Perplexity pro-search (broad synthesis) |
| `scholar "topic"` | OpenAlex + arXiv + Semantic Scholar search |
| `scholar cite DOI` | Citation graph traversal |
| `scholar fetch ARXIV_ID` | Download + convert to markdown |
| `wiki "concept"` | Wikipedia lookup (orientation only) |
| `browse "url"` | LLM-driven web automation |
| `ingest-doc file.pdf` | PDF/DOCX extraction to markdown |

### System

| Command | Purpose |
|---------|---------|
| `assemble-context --nous X` | Compile session context for agent |
| `compile-context` | Regenerate workspace files from templates |
| `distill --nous X --text "..."` | Extract structured insights |
| `aletheia-graph query "..."` | Knowledge graph CLI |
| `attention-check --nous X` | Adaptive awareness scoring |
| `deliberate "question"` | Cross-agent PROPOSE/CRITIQUE/SYNTHESIZE |
| `compose-team "task"` | Dynamic agent composition |
| `checkpoint save/restore/list` | System state management |

### Agent Management

| Command | Purpose |
|---------|---------|
| `nous-health` | Agent health check |
| `nous-contracts show AGENT` | Display agent contract |
| `nous-contracts route "request"` | Route request to appropriate agent |
| `audit-all-nous` | Full audit across all agents |
| `trace-session --stats` | Langfuse session statistics |

---

## Deployment

### Prerequisites

- Node.js >=22.12
- pnpm 10.x
- Docker (for signal-cli, Langfuse)
- signal-cli configured with a registered phone number

### Systemd Service

The gateway runs as `aletheia.service` under the `syn` service account.

```bash
# Service management
sudo systemctl status aletheia
sudo systemctl restart aletheia
journalctl -u aletheia -f

# Config location
/home/syn/.aletheia/aletheia.json
```

**Known issue**: `systemctl restart` can leave an orphan gateway process holding port 18789. If the service fails to bind on restart, check for and kill the orphan process.

### Docker Services

**signal-cli** (main docker-compose.yml):
```bash
docker compose up -d signal-cli
```
- JSON-RPC mode on port 8080 (localhost only)
- Data volume: `/mnt/ssd/aletheia/signal-cli/`

**Memory stack** (infrastructure/memory/docker-compose.yml):
```bash
cd infrastructure/memory && docker compose up -d
```
- Qdrant: vector store on port 6333
- Neo4j: graph store on port 7474 (browser) / 7687 (bolt)
- Mem0 sidecar: `aletheia-memory.service` on port 8230

**Langfuse** (infrastructure/langfuse/docker-compose.yml):
```bash
cd infrastructure/langfuse && docker compose up -d
```
- Web UI on port 3100
- PostgreSQL backend (Alpine, mem-limited 256MB)
- Telemetry disabled

### Environment

Environment variables are injected via the `ALETHEIA PATCH` in `server.impl.js`. The `aletheia.env` file uses `export` syntax (not compatible with systemd `EnvironmentFile`).

Key env vars: `ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `ALETHEIA_*` namespace.

---

## Configuration

| File | Purpose |
|------|---------|
| `/home/syn/.aletheia/aletheia.json` | Gateway config (agents, bindings, routing, sessions) |
| `shared/config/tools.yaml` | Tool definitions (source of truth for TOOLS.md) |
| `shared/config/provider-failover.json` | Multi-provider LLM failover rules |
| `infrastructure/memory/sidecar/aletheia_memory/config.py` | Mem0 backend configuration |
| `shared/contracts/*.json` | Per-agent capability contracts |
| `shared/templates/` | Template sections + per-agent YAML for compiled workspace files |

**Config reload**: Bindings, agents, routing, and session configs are `kind: "none"` -- SIGUSR1 will not reload them. Changes require a full `systemctl restart aletheia`.

---

## Development

### Building the Runtime

```bash
cd infrastructure/runtime
pnpm install
pnpm build          # tsdown compile + plugin SDK + build info
```

### Testing

```bash
pnpm test           # Parallel test runner
pnpm test:fast      # Unit tests only (vitest)
pnpm test:e2e       # End-to-end tests
pnpm test:live      # Live model tests (requires API keys)
```

### Runtime Patches

The runtime is a fork of OpenClaw with local patches:

| Patch | Purpose |
|-------|---------|
| Structured distillation | Pre-compaction extracts facts to JSONL + graph |
| Context assembly | Session start compiles state + facts + graph + tasks |
| Adaptive awareness | Prosoche replaces static heartbeats |
| Environment injection | `ALETHEIA_*` vars available to all scripts |
| Post-compaction hooks | Fire-and-forget distillation after context compression |
| Workspace dynamic context | Dynamic context injection per agent workspace |
| Mem0 memory integration | Federated search (sqlite-vec + Mem0), compaction context passthrough |

Patches live in `infrastructure/patches/` and are applied via `patch-runtime` (in `shared/bin/`).

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
