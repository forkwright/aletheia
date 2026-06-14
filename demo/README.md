# Demo workspace

A portable demo instance that runs with a local LLM server. No cloud account, no API key, no external services required beyond the LLM server.

## Prerequisites

| Tool | Version | Install |
|------|---------|---------|
| `aletheia` | any | `cargo build --release && cp target/release/aletheia ~/.local/bin/` |
| Ollama (or compatible) | any | [ollama.com](https://ollama.com) — or substitute any OpenAI-compatible server |
| A pulled model | any | `ollama pull llama3.2` |
| `curl`, `jq` | any | system package manager |

The demo uses `auth = "none"` and `provider = "mock"` for embedding — no tokens needed.

## Setup

### 1. Start a local LLM server

Using Ollama:

```bash
ollama serve
ollama pull llama3.2
```

Using llama.cpp server or any other OpenAI-compatible endpoint is also fine.

### 2. Update the model name

Edit `demo/instance/config/aletheia.toml` and replace `"demo-model"` with your
actual model name. For Ollama with llama3.2:

```toml
[[agents.list]]
# ...

[[providers]]
name = "local-demo"
providerType = "openai-compatible"
baseUrl = "http://127.0.0.1:11434/v1"
models = ["llama3.2"]  # match your ollama pull name

[agents.defaults.model]
primary = "llama3.2"
```

### 3. Run the demo

```bash
# Verify config and start the TUI
aletheia -r demo/instance serve &
aletheia -r demo/instance tui
```

Or run the automated smoke test (starts the server, checks health and agent
registration, then exits):

```bash
bash demo/smoke-test.sh
```

## What the demo exercises

- Configuration loading from `demo/instance/config/aletheia.toml`
- Agent workspace bootstrap from `demo/instance/nous/demo/`
- Server startup and health endpoint
- TUI session initiation (when run interactively)
- Smoke test: config check, server health, agent registration

## Success condition

The smoke test exits 0 and prints `--- PASS ---`.

For interactive use: the TUI connects and shows the `demo` agent ready for input.

## Customizing the demo

| File | What to change |
|------|----------------|
| `demo/instance/config/aletheia.toml` | Model name, port, provider URL |
| `demo/instance/nous/demo/SOUL.md` | Agent character |
| `demo/instance/nous/demo/GOALS.md` | Agent goals |

## Relationship to production

The demo uses `auth = "none"` and mock embedding. For production, see
[docs/QUICKSTART.md](../docs/QUICKSTART.md) and [docs/DEPLOYMENT.md](../docs/DEPLOYMENT.md).

The config uses port 18799 (not the default 18789) to avoid conflicts with a
running production instance.
