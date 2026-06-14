# Aletheia

Self-hosted AI agents with persistent memory.

Talk to an AI that remembers your previous conversations, learns your preferences, and builds a knowledge graph over time. Give it a name, a personality, and goals. Run it from a terminal dashboard, HTTP API, or Signal messenger.

One binary - no containers, no external databases, no sidecars. The only runtime cloud dependency is your LLM provider. On first run, the `candle` embedding provider downloads model files from HuggingFace Hub and caches them locally; subsequent runs are fully offline. See [NETWORK.md](docs/NETWORK.md) for every outbound call the binary makes.

Current first run: start the server and use the TUI. The desktop app is the
v1.0 target surface and can be installed as a preview from a source checkout,
but it is not the default public onboarding path yet.

[Golden Path](docs/GOLDEN-PATH.md) · [Quickstart](docs/QUICKSTART.md) · [Configuration](docs/CONFIGURATION.md) · [Deployment](docs/DEPLOYMENT.md) · [Architecture](docs/ARCHITECTURE.md) · [Demo](demo/README.md) · [Docs map](docs/DOCS-MAP.md)

---

## Install

Download the tarball from [releases](https://github.com/forkwright/aletheia/releases), extract, and run `init`:

```bash
VERSION=$(curl -s https://api.github.com/repos/forkwright/aletheia/releases/latest | grep '"tag_name":' | cut -d'"' -f4)
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
- **Working-memory continuity.** Each turn can inject agent-curated `<key_info>` from the prior working checkpoint before recall and history are assembled.
- **Multiple agents.** Each agent has its own character (SOUL.md), goals, memory, and workspace. They can coordinate, delegate, and specialize.
- **Tools.** Built-in tools cover file I/O, shell execution, web search, memory search, planning, and agent coordination. External MCP bridge support is optional; build with `cargo build -p aletheia --features mcp` when you want runtime-discovered MCP tools in the Organon tool plane. Feature-gated additions (`energeia`, `bookkeeper`, `computer-use`, `z3`) expand the tool set further. See [ARCHITECTURE.md](docs/ARCHITECTURE.md) for the current tool inventory and feature breakdown.
- **Runtime guardrails.** Tool calls carry HMAC-SHA256 receipts, loop detection combines ping-pong, no-progress, and doom-loop signals, and per-stage timeouts bound long-running turns.
- **Terminal dashboard.** Rich TUI with markdown rendering, session management, and real-time streaming.
- **Desktop preview.** Dioxus desktop app for the v1.0 workflow target; see [DESKTOP.md](docs/DESKTOP.md).
- **Signal messaging.** Talk to your agents over Signal. Messages arrive as plain conversational turns routed to the configured agent.
- **Privacy.** No telemetry, no analytics, no phone-home. Only outbound connections are to services you configure.

---

## Architecture

Single binary deployment. The substrate includes persistent sessions, Datalog-backed memory, working-memory injection, HTTP/SSE, optional runtime MCP bridging, Signal, dispatch, and a substrate canary suite. For current workspace crate count, canary scenario count, and the full dependency graph, see [ARCHITECTURE.md](docs/ARCHITECTURE.md).

---

## Naming

Every name follows a deliberate naming philosophy. Greek provides precision where English flattens: *nous* over "agent" because these are minds, not tools. *Mneme* over "store" because memory is the function, not the container. See the in-repo [standards](standards/) and [lexicon.md](docs/lexicon.md) for the naming system and full registry.

---

## Agents

Each agent has a workspace under `nous/` with character, operations, and memory files. See `instance.example/nous/_template/` for a template, [WORKSPACE_FILES.md](docs/WORKSPACE_FILES.md) for the full reference.

## Interfaces

- **TUI** - Terminal dashboard. Rich markdown rendering, session management.
- **Desktop app** - v1.0 target surface, currently installed separately from source.
- **Signal** - Inbound messages are delivered as conversational turns to the configured agent. Messages prefixed with `!` are intercepted as operator commands (see below).
- **CLI** - `aletheia help` for the full command reference.
- **API** - REST on port 18789. See [ARCHITECTURE.md](docs/ARCHITECTURE.md).

### Signal `!`-commands

Send any of these from Signal to control the agent without starting a conversation turn. Type `!help` to see the list at any time.

| Command | What it does |
|---------|-------------|
| `!help` | List all available commands |
| `!status` | Lifecycle and session info for this agent |
| `!agents` | List all running agents |
| `!whoami` | Show which agent handles this conversation |
| `!new [label]` | Signal intent to start a fresh session |
| `!end` | Close the current session |
| `!sessions` | Count sessions tracked by this agent |
| `!ping` | Round-trip liveness check |
| `!channels` | List channel providers and health |
| `!uptime` | Agent uptime and panic-boundary count |
| `!model` | Show the LLM model configured for this agent |
| `!info [agent_id]` | Detail view for an agent (default: current) |

Commands are intercepted before reaching the agent - they consume no LLM tokens. Unknown `!` commands return a helpful error listing the available set.

## Services

| Service | Port | Required |
|---------|------|----------|
| aletheia | 18789 | Yes |
| signal-cli | 8080 | For Signal |

## Privacy

No telemetry, phone-home, analytics, crash reports, or beacon requests.

Outbound connections are limited to your explicitly configured services (LLM provider, Signal) and, on first run only, HuggingFace Hub for embedding model files. Everything else stays on your machine. See [DATA.md](docs/DATA.md) for the data inventory, [NETWORK.md](docs/NETWORK.md) for every network call the binary makes.

## License

AGPL-3.0-or-later for the runtime and all crates. Apache-2.0 for SDK and client libraries (when published). See [LICENSE](LICENSE).
