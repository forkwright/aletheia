# Aletheia

Self-hosted multi-agent AI system with persistent memory, Signal messaging, and a web UI.

Privacy-first. Runs on commodity hardware. No cloud dependencies beyond your LLM API key.

[Quickstart](docs/QUICKSTART.md) · [Configuration](docs/CONFIGURATION.md) · [Architecture](docs/ARCHITECTURE.md) · [Technology](docs/TECHNOLOGY.md) · [Data](docs/DATA.md) · [Releasing](docs/RELEASING.md)

---

## What It Is

Multiple AI agents working in concert with a human operator. Each agent has character, memory, and domain expertise. They persist understanding across sessions, coordinate through shared infrastructure, and evolve through use.

Not a chatbot framework. A distributed cognition system.

## Architecture

Aletheia is being rewritten in Rust. The target is a single static binary replacing the current Node.js gateway, Python memory sidecar, and shell scripts. The TypeScript runtime still runs production while the Rust crates reach feature parity.

```
         Web UI (Svelte 5)          Signal Messenger
              |                          |
         HTTP/SSE                   signal-cli (JSON-RPC)
              |                          |
              +----------+---------------+
                         |
                  +--------------+
                  |   Gateway    |     Session management, tool execution,
                  |   (:18789)   |     message routing, context assembly
                  +--------------+
                   /    |    |   \
              Bindings (per-agent routing)
                /       |    |      \
         +------+  +------+ +------+ +------+
         | nous |  | nous | | nous | | nous |   N agents, each with:
         +------+  +------+ +------+ +------+   - SOUL.md (character)
            |         |         |        |       - workspace files
            v         v         v        v       - persistent memory
         Claude     Claude    Claude   Claude
```

### Rust Crates (target)

See [ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full dependency graph and trait boundaries.

| Layer | Crates |
|-------|--------|
| Leaf | `koina` (errors, tracing), `symbolon` (auth), `mneme-engine` (embedded DB) |
| Low | `taxis` (config), `hermeneus` (LLM), `mneme` (memory), `organon` (tools), `agora` (channels), `melete` (distillation) |
| Mid | `nous` (agent pipeline) |
| High | `pylon` (HTTP gateway) |
| Top | `aletheia` (binary entrypoint) |

### TypeScript Runtime (current production)

14 modules following the same Greek naming: `koina`, `taxis`, `mneme`, `hermeneus`, `organon`, `nous`, `melete`, `symbolon`, `dianoia`, `semeion`, `pylon`, `prostheke`, `daemon`, `portability`.

**Models:** Anthropic (OAuth or API key). Complexity-based routing.
**Memory:** Mem0 (Qdrant + Neo4j + Haiku extraction) for cross-agent long-term memory. SQLite for sessions.
**Observability:** Optional self-hosted Langfuse.

---

## Quick Start

```bash
git clone https://github.com/forkwright/aletheia.git && cd aletheia
./setup.sh        # builds, installs CLI, opens browser
aletheia start    # from next time on
```

[Full setup guide](docs/QUICKSTART.md) · [Production deployment](docs/DEPLOYMENT.md)

---

## Why Greek?

Every name follows a deliberate naming philosophy. Greek provides precision where English flattens: *nous* over "agent" because these are minds, not tools. *Mneme* over "store" because memory is the function, not the container. Module and crate names use Greek terms reflecting their purpose (nous = mind, mneme = memory, hermeneus = interpreter). See [ALETHEIA.md](ALETHEIA.md) for philosophical grounding.

---

## Agents

Each agent has a workspace under `nous/` with character, operations, and memory files. See `nous/_example/` for a template, [WORKSPACE_FILES.md](docs/WORKSPACE_FILES.md) for the full reference.

## Interfaces

- **Web UI** - Svelte 5 at `/ui`. Streaming, file upload, syntax highlighting, thinking visualization.
- **Signal** - 15 `!` commands. `!help` for the list.
- **CLI** - `aletheia help` for the full command reference.
- **API** - REST on port 18789. See [ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Services

| Service | Port | Required |
|---------|------|----------|
| aletheia | 18789 | Yes |
| signal-cli | 8080 | For Signal |
| aletheia-memory | 8230 | Recommended |
| qdrant | 6333 | If using Mem0 |
| neo4j | 7474/7687 | If using Mem0 |
| langfuse | 3100 | Optional |

## License

AGPL-3.0 (runtime) + Apache-2.0 (SDK/client). See [LICENSING.md](LICENSING.md).
