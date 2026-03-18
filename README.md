# Aletheia

Self-hosted AI agents with persistent memory.

Talk to an AI that remembers your previous conversations, learns your preferences, builds a knowledge graph over time, and gets better the more you use it. Give it a name, a personality, goals, and tools. Run it from a terminal dashboard, HTTP API, or Signal messenger.

One binary. No containers. No external databases. No cloud dependencies beyond your LLM provider.

[Quickstart](docs/QUICKSTART.md) · [Configuration](docs/CONFIGURATION.md) · [Deployment](docs/DEPLOYMENT.md) · [Architecture](docs/ARCHITECTURE.md)

---

## Quick start

```bash
# Install (Linux x86_64)
curl -L https://github.com/forkwright/aletheia/releases/latest/download/aletheia-linux-x86_64 -o aletheia
chmod +x aletheia && sudo mv aletheia /usr/local/bin/

# Setup (interactive wizard handles config, credentials, first agent)
aletheia init

# Run
aletheia                # start the server
aletheia tui            # open the terminal dashboard
```

Build from source: `cargo build --release` (requires Rust 1.94+). See [QUICKSTART.md](docs/QUICKSTART.md) for the full guide.

---

## What you get

- **Persistent memory.** Conversations carry forward. The agent builds a knowledge graph of facts, entities, and relationships that persists across sessions and grows over time.
- **Multiple agents.** Each agent has its own character (SOUL.md), goals, memory, and workspace. They can coordinate, delegate, and specialize.
- **Tools.** 33 built-in tools: file I/O, shell execution, web search, memory search, agent coordination. Extend with domain packs or custom tools.
- **Terminal dashboard.** Rich TUI with markdown rendering, session management, and real-time streaming.
- **Signal messaging.** Talk to your agents over Signal with 15 built-in commands.
- **Privacy.** No telemetry, no analytics, no phone-home. Only outbound connections are to services you configure.

---

## Architecture

Rust workspace with 18 crates. Single binary deployment.

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

### Rust crates (target)

See [ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full dependency graph and trait boundaries.

| Layer | Crates |
|-------|--------|
| Leaf | `koina` (errors, tracing), `symbolon` (auth) |
| Low | `taxis` (config), `hermeneus` (LLM), `mneme` (memory + embedded DB engine), `organon` (tools), `agora` (channels), `melete` (distillation) |
| Mid | `nous` (agent pipeline) |
| High | `pylon` (HTTP gateway) |
| Top | `aletheia` (binary entrypoint) |

**Models:** Anthropic (OAuth or API key). Complexity-based routing.
**Memory:** Embedded Datalog+HNSW engine for knowledge graph, long-term memory, and sessions.
**Observability:** Structured tracing with tokio-tracing.

---

## Quick start

```bash
git clone https://github.com/forkwright/aletheia.git && cd aletheia
cargo build --release
cp target/release/aletheia ~/.local/bin/
aletheia
```

[Full setup guide](docs/QUICKSTART.md) · [Production deployment](docs/DEPLOYMENT.md)

---

## Naming

Every name follows a deliberate naming philosophy. Greek provides precision where English flattens: *nous* over "agent" because these are minds, not tools. *Mneme* over "store" because memory is the function, not the container. See [gnomon.md](docs/gnomon.md) for the naming system and [lexicon.md](docs/lexicon.md) for the full registry.

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
