# Aletheia

*Self-hosted multi-agent AI system with a web UI, persistent memory, and Signal messaging.*

Privacy-first. Runs on commodity hardware as a systemd service. No cloud dependencies beyond your LLM API key.

**v0.10.0** | [Quickstart](docs/QUICKSTART.md) | [Configuration](docs/CONFIGURATION.md) | [Development](docs/DEVELOPMENT.md)

---

## Architecture

```
         Web UI (Svelte 5)          Signal Messenger
              |                          |
         HTTP/SSE (:18789/ui)       signal-cli (JSON-RPC, :8080)
              |                          |
              +----------+---------------+
                         |
                  +--------------+
                  |   Aletheia   |     Node.js gateway (TypeScript/tsdown)
                  |   Gateway    |     Session management, tool execution,
                  |   (:18789)   |     message routing, context assembly
                  +--------------+
                   /    |    |   \
              Bindings (per-agent routing)
                /       |    |      \
         +------+  +------+ +------+ +------+
         | agent|  | agent| | agent| | agent|   N agents, each with:
         +------+  +------+ +------+ +------+   - SOUL.md (character)
            |         |         |        |       - AGENTS.md (operations)
            v         v         v        v       - MEMORY.md (continuity)
         Claude     Claude    Claude   Claude
```

**Runtime**: Node.js >=22.12, TypeScript compiled with tsdown (~450KB bundle), Hono gateway on port 18789.

**Interfaces**: Svelte 5 web UI with streaming, file upload, syntax highlighting, thinking visualization, and force-directed memory graph. Signal messenger via signal-cli. 14 built-in `!` commands. CLI admin tools.

**Models**: Anthropic (OAuth or API key). Complexity-based routing. Extended thinking for reasoning models.

**Memory**: Mem0 (Qdrant vectors + Neo4j graph + Claude Haiku extraction) for cross-agent long-term memory. sqlite-vec for local per-agent search. Working state extraction and agent notes survive distillation. Cross-agent blackboard.

**Observability**: Self-hosted Langfuse (port 3100) for session traces and metrics.

---

## Directory Structure

```
aletheia/
├── nous/                   Agent workspaces (SOUL.md, MEMORY.md, etc.)
│   └── _example/           Template workspace
├── shared/                 Common scripts, templates, config, skills
├── infrastructure/
│   ├── runtime/            Gateway (TypeScript/tsdown)
│   │   └── src/
│   │       ├── taxis/          Config loading + validation (Zod)
│   │       ├── mneme/          Session store (better-sqlite3, 10 migrations)
│   │       ├── hermeneus/      Anthropic SDK + provider router
│   │       ├── organon/        Tool registry + 41 built-in tools + skills
│   │       ├── semeion/        Signal client, listener, commands
│   │       ├── pylon/          Hono HTTP gateway, MCP, Web UI
│   │       ├── prostheke/      Plugin system
│   │       ├── nous/           Agent bootstrap + turn pipeline
│   │       ├── distillation/   Context summarization
│   │       ├── daemon/         Cron, watchdog, update checker
│   │       └── koina/          Shared utilities
│   ├── memory/             Mem0 sidecar + docker-compose (Qdrant, Neo4j)
│   ├── langfuse/           Self-hosted observability
│   └── prosoche/           Adaptive attention daemon
├── ui/                     Web UI (Svelte 5)
└── config/                 Example configuration
```

---

## Agents

Each agent has a workspace under `nous/` with character (`SOUL.md`), operations (`AGENTS.md`), and memory (`MEMORY.md`). See `nous/_example/` for a template and [WORKSPACE_FILES.md](docs/WORKSPACE_FILES.md) for the full reference.

---

## Interfaces

### Web UI

Svelte 5 at `/ui`. Streaming responses, file upload, syntax highlighting, thinking pills, force-directed memory graph.

### Signal Commands

`!ping` `!help` `!status` `!sessions` `!reset` `!agent` `!skills` `!model` `!think` `!distill` `!blackboard` `!approve` `!deny` `!contacts`

### CLI

| Command | Purpose |
|---------|---------|
| `aletheia gateway` | Start the runtime |
| `aletheia status` | System status |
| `aletheia send -a <agent> -m "..."` | Send message to agent |
| `aletheia sessions [-a <agent>]` | List sessions |
| `aletheia cron list\|trigger <id>` | Manage cron jobs |
| `aletheia update [version]` | Self-update with rollback |
| `aletheia doctor` | Connectivity checks |

### API

Full REST API on port 18789. Key endpoints:

- `/health` — health check
- `/api/status` — agent list + version
- `/api/agents` — all agents with model info
- `/api/sessions/stream` — streaming message (SSE)
- `/api/costs/summary` — token usage and cost
- `/api/metrics` — full system metrics
- `/api/events` — SSE event stream

See [DEVELOPMENT.md](docs/DEVELOPMENT.md) for full endpoint list.

---

## Services

| Service | Port | Required |
|---------|------|----------|
| aletheia | 18789 | Yes |
| signal-cli | 8080 | Yes |
| aletheia-memory | 8230 | Recommended |
| qdrant | 6333 | If using Mem0 |
| neo4j | 7474/7687 | If using Mem0 |
| langfuse | 3100 | Optional |

---

## Quick Start

```bash
git clone https://github.com/forkwright/aletheia.git && cd aletheia
cd infrastructure/runtime && npm install && npx tsdown && cd ../..
cd ui && npm install && npm run build && cd ..
cp .env.example shared/config/aletheia.env  # Edit: API keys, paths
cp config/aletheia.example.json ~/.aletheia/aletheia.json  # Edit: agents, bindings
cp -r nous/_example nous/your-agent  # Edit: SOUL.md, IDENTITY.md
node infrastructure/runtime/aletheia.mjs gateway
```

See [QUICKSTART.md](docs/QUICKSTART.md) for full setup, [DEPLOYMENT.md](docs/DEPLOYMENT.md) for production, [RESCUE.md](RESCUE.md) for recovery.

---

## License

AGPL-3.0 (runtime) + Apache-2.0 (SDK/client). See [LICENSING.md](LICENSING.md).

*Built by forkwright, 2026*
