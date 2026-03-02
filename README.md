# Ergon

Multi-agent AI platform with persistent memory, messaging, and a web interface. Fork of [forkwright/aletheia](https://github.com/forkwright/aletheia).

Self-hosted. Runs on commodity hardware. No cloud dependencies beyond an LLM API key.

**v0.10.0** | [Quickstart](docs/QUICKSTART.md) | [Configuration](docs/CONFIGURATION.md) | [Development](docs/DEVELOPMENT.md)

---

## Architecture

Ergon is built on Aletheia's runtime. The TypeScript gateway runs production today; a Rust rewrite is in progress for single-binary deployment.

### Runtime Stack

```
         Web UI (Svelte 5)          Signal Messenger
              |                          |
         HTTP/SSE (:18789/ui)       signal-cli (JSON-RPC, :8080)
              |                          |
              +----------+---------------+
                         |
                  +--------------+
                  |    Ergon     |     Node.js gateway (TypeScript/tsdown)
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

**Interfaces**: Svelte 5 web UI with streaming, file browser, and syntax highlighting. Signal messenger via signal-cli. CLI admin tools.

**Models**: Anthropic (OAuth or API key). Complexity-based routing across model tiers.

**Memory**: Dual-layer — Mem0 (Qdrant vectors + Neo4j graph + LLM extraction) for cross-agent long-term memory; sqlite-vec for local per-agent search.

### Rust Crates (in progress)

```
ergon (binary)
├── koina           shared errors, tracing, utilities
├── taxis           config loading, path resolution
├── mneme           unified memory store (sqlite + fastembed + CozoDB)
├── mneme-engine    CozoDB embedded database (vectors, graph, relations)
├── hermeneus       Anthropic client, model routing, credential management
├── organon         tool registry, built-in tools
├── nous            agent pipeline, actor model (tokio)
├── melete          context distillation, compression strategies
├── agora           channel registry, Signal/Slack providers
├── pylon           Axum HTTP gateway, SSE streaming
└── symbolon        JWT authentication, session management, RBAC
```

11 crates, 718 tests, ~21K lines of Rust. See [PROJECT.md](docs/PROJECT.md) for status.

---

## Directory Structure

```
ergon/
├── crates/                     Rust workspace
├── infrastructure/
│   ├── runtime/                Gateway (TypeScript) — current production
│   ├── memory/                 Mem0 sidecar + Qdrant + Neo4j
│   ├── prosoche/               Adaptive attention daemon
│   └── langfuse/               Self-hosted observability
├── instance/                   Deployment-specific state
│   ├── nous/                   Agent workspaces (SOUL.md, MEMORY.md, etc.)
│   ├── config/                 Runtime config
│   └── data/                   Session DB, logs
├── ui/                         Web UI (Svelte 5)
├── shared/                     Scripts, templates, hooks
└── config/                     Example configuration
```

---

## Agents

Each agent has a workspace under `instance/nous/` with identity (`SOUL.md`), operations (`AGENTS.md`), and continuity (`MEMORY.md`). See [WORKSPACE_FILES.md](docs/WORKSPACE_FILES.md) for the full reference.

---

## Quick Start

```bash
git clone https://github.com/CKickertz/ergon.git && cd ergon
./setup.sh
```

Setup builds the runtime and UI, creates a default config, and opens your browser at `http://localhost:18789`.

After first run:

| Task | Command |
|------|---------|
| Start | `aletheia start` |
| Stop | `aletheia stop` |
| Status | `aletheia status` |
| Logs | `aletheia logs -f` |
| Diagnose | `aletheia doctor` |

See [QUICKSTART.md](docs/QUICKSTART.md) for details, [DEPLOYMENT.md](docs/DEPLOYMENT.md) for production setup.

---

## Services

| Service | Port | Required |
|---------|------|----------|
| Gateway | 18789 | Yes |
| Signal | 8080 | For messaging |
| Memory sidecar | 8230 | Recommended |
| Qdrant | 6333 | If using Mem0 |
| Neo4j | 7474/7687 | If using Mem0 |

---

## Upstream

Ergon is a fork of [forkwright/aletheia](https://github.com/forkwright/aletheia). Internal module names (koina, taxis, mneme, hermeneus, etc.) are preserved from upstream — see [ALETHEIA.md](ALETHEIA.md) for the naming philosophy.

See [fork-upstream.md](docs/policy/fork-upstream.md) for the sync policy: what stays upstream, what's fork-specific, and how conflicts are resolved.

## License

AGPL-3.0 (runtime) + Apache-2.0 (SDK/client). See [LICENSING.md](LICENSING.md).
