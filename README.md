# Aletheia

Self-hosted AI agents with persistent memory.

Talk to an AI that remembers your previous conversations, learns your preferences, and builds a knowledge graph over time. Give it a name, a personality, and goals. Run it from a terminal dashboard, HTTP API, or Signal messenger.

One binary. No containers. No external databases. No cloud dependencies beyond your LLM provider.

[Quickstart](docs/QUICKSTART.md) · [Configuration](docs/CONFIGURATION.md) · [Deployment](docs/DEPLOYMENT.md) · [Architecture](docs/ARCHITECTURE.md)

---

## Install

Download the tarball from [releases](https://github.com/forkwright/aletheia/releases), extract, and run `init`:

```bash
VERSION=v0.13.11
curl -L "https://github.com/forkwright/aletheia/releases/download/${VERSION}/aletheia-linux-x86_64-${VERSION}.tar.gz" \
  -o aletheia.tar.gz
tar xzf aletheia.tar.gz
cd "aletheia-${VERSION}"
sudo cp aletheia /usr/local/bin/
aletheia init
```

The tarball contains `instance.example/` with the reference config layout. See [QUICKSTART.md](docs/QUICKSTART.md) for full install, macOS, and source build instructions.

---

## What you get

- **Persistent memory.** Conversations carry forward. The agent builds a knowledge graph of facts, entities, and relationships that persists across sessions and grows over time.
- **Multiple agents.** Each agent has its own character (SOUL.md), goals, memory, and workspace. They can coordinate, delegate, and specialize.
- **Tools.** 38 built-in tools: file I/O, shell execution, web search, memory search, planning, agent coordination. Extend with domain packs or custom tools.
- **Terminal dashboard.** Rich TUI with markdown rendering, session management, and real-time streaming.
- **Signal messaging.** Talk to your agents over Signal with 15 built-in commands.
- **Privacy.** No telemetry, no analytics, no phone-home. Only outbound connections are to services you configure.

---

## Architecture

Rust workspace with 23 crates. Single binary deployment. See [ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full dependency graph and trait boundaries.

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
