# Aletheia

Self-hosted multi-agent AI system with persistent memory, Signal messaging, and a web UI.

Privacy-first. Runs on commodity hardware. No cloud dependencies beyond your LLM API key.

**v0.10.0** | [Quickstart](docs/QUICKSTART.md) | [Configuration](docs/CONFIGURATION.md) | [Development](docs/DEVELOPMENT.md)

---

## Architecture

Aletheia is being rewritten in Rust. The target is a single static binary that replaces the current Node.js gateway, Python memory sidecar, and all shell scripts with one `scp + systemctl` deployment. The TypeScript runtime still runs production today while the Rust crates reach feature parity.

### Rust Crates (in progress)

```
aletheia (binary)
├── koina           shared errors (snafu), tracing, fs utilities
├── taxis           config loading (figment YAML cascade), path resolution, oikos hierarchy
├── mneme           unified memory store (sqlite + fastembed + optional CozoDB)
├── mneme-engine    CozoDB embedded database (vectors, graph, relations, bi-temporal facts)
├── hermeneus       Anthropic client, model routing, credential management
├── organon         tool registry, built-in tool definitions
├── nous            agent pipeline, actor model (tokio), bootstrap, recall, finalize
├── melete          context distillation, compression strategies, token budgets
├── agora           channel registry, ChannelProvider trait, Signal provider (semeion)
├── pylon           Axum HTTP gateway, SSE streaming, static UI serving
└── symbolon        JWT authentication, session management, RBAC
```

11 application crates, 718 tests, ~21K lines of Rust. See [PROJECT.md](docs/PROJECT.md) for milestone status and the full roadmap.

### TypeScript Runtime (current production)

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

**Interfaces**: Svelte 5 web UI with streaming, file upload, syntax highlighting, and thinking visualization. Signal messenger via signal-cli. 15 built-in `!` commands. CLI admin tools.

**Models**: Anthropic (OAuth or API key). Complexity-based routing. Extended thinking for reasoning models.

**Memory**: Mem0 (Qdrant vectors + Neo4j graph + Claude Haiku extraction) for cross-agent long-term memory. sqlite-vec for local per-agent search. Working state extraction and agent notes survive distillation. Cross-agent blackboard.

**Observability**: Self-hosted Langfuse (port 3100) for session traces and metrics.

---

## Directory Structure

```
aletheia/
├── crates/                     Rust workspace (the rewrite)
│   ├── koina/                  Shared errors, tracing, utilities
│   ├── taxis/                  Config loading, path resolution
│   ├── mneme/                  Unified memory store
│   ├── mneme-engine/           CozoDB embedded database
│   ├── hermeneus/              Anthropic client, model routing
│   ├── organon/                Tool registry
│   ├── nous/                   Agent pipeline, actor model
│   ├── melete/                 Distillation, reflection
│   ├── agora/                  Channel registry, Signal provider
│   ├── pylon/                  Axum HTTP gateway
│   ├── symbolon/               Authentication, RBAC
│   ├── graph-builder/          Build-time dependency graph visualization
│   └── integration-tests/      Cross-crate integration tests
│
├── nous/                       Agent workspaces (SOUL.md, MEMORY.md, etc.)
│   └── _example/               Template workspace
├── shared/                     Common scripts, templates, config, skills
├── infrastructure/
│   ├── runtime/                Gateway (TypeScript/tsdown) -- current production
│   │   └── src/
│   │       ├── taxis/          Config loading + validation (Zod)
│   │       ├── mneme/          Session store (better-sqlite3, 10 migrations)
│   │       ├── hermeneus/      Anthropic SDK + provider router
│   │       ├── organon/        Tool registry + 48 built-in tools + skills
│   │       ├── semeion/        Signal client, listener, commands
│   │       ├── pylon/          Hono HTTP gateway, MCP, Web UI
│   │       ├── nous/           Agent bootstrap + turn pipeline
│   │       ├── melete/         Distillation, reflection, memory flush
│   │       ├── symbolon/       Split-token authentication
│   │       ├── dianoia/        Multi-phase planning orchestrator
│   │       ├── daemon/         Cron, watchdog, update checker
│   │       └── koina/          Shared utilities
│   ├── memory/                 Mem0 sidecar + docker-compose (Qdrant, Neo4j)
│   ├── langfuse/               Self-hosted observability
│   └── prosoche/               Adaptive attention daemon
├── ui/                         Web UI (Svelte 5)
└── config/                     Example configuration
```

---

## Why Greek?

Every name in this system follows a deliberate naming philosophy. Names unconceal essential natures, not describe implementations. Greek provides the precision: where English has "knowledge," Greek distinguishes between episteme, gnosis, techne, phronesis, and nous, each a fundamentally different stance toward knowing. Where English has "form," Greek has morphe, schema, eidos, each a different structural relationship.

See **[docs/gnomon.md](docs/gnomon.md)** for the full naming system, including the layer test, dimensional resonance, and the process for naming new components.

---

## Agents

Each agent has a workspace under `nous/` with character (`SOUL.md`), operations (`AGENTS.md`), and memory (`MEMORY.md`). See `nous/_example/` for a template and [WORKSPACE_FILES.md](docs/WORKSPACE_FILES.md) for the full file reference.

---

## Interfaces

### Web UI

Svelte 5 at `/ui`. Streaming responses, file upload, syntax highlighting, thinking pills, force-directed memory graph.

### Signal Commands

`!ping` `!help` `!status` `!sessions` `!reset` `!agent` `!skills` `!model` `!think` `!distill` `!blackboard` `!approve` `!deny` `!contacts`

### CLI

| Command | Purpose |
|---------|---------|
| `aletheia start` | Start memory services + gateway, open browser |
| `aletheia stop [--all]` | Stop gateway (--all includes memory containers) |
| `aletheia restart` | Stop then start |
| `aletheia logs [-f]` | View gateway logs |
| `aletheia tui` | Launch terminal UI |
| `aletheia status` | System health and agent list |
| `aletheia doctor [--fix]` | Validate config and connectivity |
| `aletheia send -a <agent> -m "..."` | Send message to agent |
| `aletheia sessions [-a <agent>]` | List sessions |
| `aletheia cron list\|trigger <id>` | Manage cron jobs |
| `aletheia update [version]` | Self-update with rollback |

### API

Full REST API on port 18789. Key endpoints:

- `/health` -- health check
- `/api/status` -- agent list + version
- `/api/agents` -- all agents with model info
- `/api/sessions/stream` -- streaming message (SSE)
- `/api/costs/summary` -- token usage and cost
- `/api/metrics` -- full system metrics
- `/api/events` -- SSE event stream

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
./setup.sh        # builds, installs CLI, opens browser
aletheia start    # from next time on
```

See [QUICKSTART.md](docs/QUICKSTART.md) for full setup, [DEPLOYMENT.md](docs/DEPLOYMENT.md) for production, [RESCUE.md](RESCUE.md) for recovery.

---

## License

AGPL-3.0 (runtime) + Apache-2.0 (SDK/client). See [LICENSING.md](LICENSING.md).

*Built by forkwright, 2026*