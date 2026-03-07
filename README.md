# Aletheia

Self-hosted multi-agent AI system with persistent memory and Signal messaging.

Privacy-first. Runs on commodity hardware. No cloud dependencies beyond your LLM API key.

[Quickstart](docs/QUICKSTART.md) · [Configuration](docs/CONFIGURATION.md) · [Architecture](docs/ARCHITECTURE.md) · [Architecture Guide](docs/ARCHITECTURE-GUIDE.md) · [Technology](docs/TECHNOLOGY.md) · [Data](docs/DATA.md) · [Network](docs/NETWORK.md) · [Packs](docs/PACKS.md) · [Vendoring](docs/VENDORING.md) · [Releasing](docs/RELEASING.md)

---

## What It Is

Multiple AI agents working in concert with a human operator. Each agent has character, memory, and domain expertise. They persist understanding across sessions, coordinate through shared infrastructure, and evolve through use.

Not a chatbot framework. A distributed cognition system.

## Architecture

Rust workspace with 16 crates. Single binary deployment.

```text
     TUI / HTTP API              Signal Messenger
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
| Leaf | `koina` (errors, tracing), `symbolon` (auth) |
| Low | `taxis` (config), `hermeneus` (LLM), `mneme` (memory + embedded DB engine), `organon` (tools), `agora` (channels), `melete` (distillation) |
| Mid | `nous` (agent pipeline) |
| High | `pylon` (HTTP gateway) |
| Top | `aletheia` (binary entrypoint) |

**Models:** Anthropic (OAuth or API key). Complexity-based routing.
**Memory:** CozoDB embedded for knowledge graph, long-term memory, and sessions.
**Observability:** Structured tracing with tokio-tracing.

---

## Quick Start

```bash
git clone https://github.com/forkwright/aletheia.git && cd aletheia
cargo build --release
cp target/release/aletheia ~/.local/bin/
aletheia
```

[Full setup guide](docs/QUICKSTART.md) · [Production deployment](docs/DEPLOYMENT.md)

---

## Why Greek?

Every name follows a deliberate naming philosophy. Greek provides precision where English flattens: *nous* over "agent" because these are minds, not tools. *Mneme* over "store" because memory is the function, not the container. See [ALETHEIA.md](docs/ALETHEIA.md) for the manifesto, [gnomon.md](docs/gnomon.md) for the naming system.

---

## Agents

Each agent has a workspace under `nous/` with character, operations, and memory files. See `instance.example/nous/_template/` for a template, [WORKSPACE_FILES.md](docs/WORKSPACE_FILES.md) for the full reference.

## Interfaces

- **TUI** - Terminal dashboard. Rich markdown rendering, session management.
- **Signal** - 15 `!` commands. `!help` for the list.
- **CLI** - `aletheia help` for the full command reference.
- **API** - REST on port 18789. See [ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Services

| Service | Port | Required |
|---------|------|----------|
| aletheia | 18789 | Yes |
| signal-cli | 8080 | For Signal |

## Privacy

No telemetry. No phone-home. No analytics, crash reports, or beacon requests.

The only outbound connections are to services you explicitly configure (LLM provider, Signal). Everything else stays on your machine. See [DATA.md](docs/DATA.md) for the data inventory, [NETWORK.md](docs/NETWORK.md) for every network call the binary makes.

## License

AGPL-3.0-or-later for the runtime and all crates. Apache-2.0 for SDK and client libraries (when published). See [LICENSE](LICENSE).
