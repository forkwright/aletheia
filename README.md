# Aletheia

Self-hosted AI agents with persistent memory.

Talk to an AI that remembers your previous conversations, learns your preferences, and builds a knowledge graph over time. Give it a name, a personality, and goals. Run it from a terminal dashboard, HTTP API, or Signal messenger.

One binary - no containers, no external databases, no sidecars. The only runtime cloud dependency is your LLM provider. On first run, the `candle` embedding provider downloads model files from HuggingFace Hub and caches them locally; subsequent runs are fully offline. See [NETWORK.md](docs/NETWORK.md) for every outbound call the binary makes.

Current first run: start the server and use the TUI. The desktop app is the
v1.0 target surface and can be installed as a preview from a source checkout,
but it is not the default public onboarding path yet.

[Golden Path](docs/GOLDEN-PATH.md) · [Quickstart](docs/QUICKSTART.md) · [Configuration](docs/CONFIGURATION.md) · [Deployment](docs/DEPLOYMENT.md) · [Architecture](docs/ARCHITECTURE.md) · [Harness Lifecycle](docs/HARNESS-LIFECYCLE.md) · [UX State Inventory](docs/UX-STATE-INVENTORY.md) · [Maturity](docs/MATURITY.md) · [Demo](demo/README.md) · [Docs index](docs/MANIFEST.toml)

---

## Install

Download the tarball from [releases](https://github.com/forkwright/aletheia/releases), extract, and run `init`:

```bash
TAG=$(curl -fsSL https://api.github.com/repos/forkwright/aletheia/releases/latest | grep '"tag_name":' | cut -d'"' -f4)
VERSION="${TAG#v}"
TARBALL="aletheia-linux-x86_64-${VERSION}.tar.gz"
curl -fLO "https://github.com/forkwright/aletheia/releases/download/${TAG}/${TARBALL}"
curl -fLO "https://github.com/forkwright/aletheia/releases/download/${TAG}/${TARBALL}.sha256"
sha256sum -c "${TARBALL}.sha256"
tar xzf "$TARBALL"
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

Every name follows a deliberate naming philosophy. Greek provides precision where English flattens: *nous* over "agent" because these are minds, not tools. *Mneme* over "store" because memory is the function, not the container. See the kanon-canonical standards (`forkwright/kanon` at `crates/basanos/standards/`) and [lexicon.md](docs/lexicon.md) for the naming system and full registry.

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

`!help` and the fixed `!ping` response are available from any routed
conversation. Operational commands are shown and accepted only when the
selected route proves an exact account-scoped direct-message principal with
`sourceKind = "direct"` and `commandTier = "operator"`.

| Command | Authority | What it does |
|---------|-----------|--------------|
| `!help` | Public | List commands available to this route |
| `!ping` | Public | Fixed liveness response (`Pong.`) |
| `!status` | Operator | Lifecycle and session info for this agent |
| `!agents` | Operator | List all running agents |
| `!whoami` | Operator | Show which agent handles this conversation |
| `!sessions` | Operator | Count sessions tracked by this agent |
| `!channels` | Operator | List channel providers and health |
| `!uptime` | Operator | Agent uptime and panic-boundary count |
| `!model` | Operator | Show the LLM model configured for this agent |
| `!skills` | Operator | List skills available to this agent |
| `!blackboard` | Operator | Show recent cross-nous blackboard entries |
| `!think` | Operator | Show extended-thinking mode and budget |
| `!info [agent_id]` | Operator | Detail view for an agent (default: current) |

Commands are intercepted before reaching the agent and consume no LLM tokens.
Unknown and denied commands receive the same fixed `Unknown command.` reply;
unknown names and arguments are neither echoed nor durably audited.

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

<!-- kanon:auto-start -->
## Repository Metadata

- Registry name: `aletheia`
- Description: Kanon-managed forkwright repository `aletheia`.
- Forge repo: `forkwright/aletheia`
- Kanon prefix: `al`
- Config source: `workflow/kanon.toml [projects.aletheia]`
- Planning state: `projects/aletheia/STATE.md`
- Last state update: `not recorded`

Run `kanon docs sync --check --repo aletheia` to verify this generated
section and `kanon docs sync --apply --repo aletheia` to refresh it.

## Blast zone

- Paths explicitly named by the rendered prompt, role, or template input.

## Acceptance verifier

```bash
kanon gate
```
<!-- kanon:auto-end -->
