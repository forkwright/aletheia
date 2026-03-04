# Quickstart

## Prerequisites

- Node.js >= 22.12
- An Anthropic API key (or Claude Code installed - key is auto-detected)

## Install

```bash
git clone https://github.com/forkwright/aletheia.git
cd aletheia
./setup.sh
```

This builds the runtime and UI, installs the `aletheia` CLI to `~/.local/bin`, starts the gateway, and opens your browser. Follow the setup wizard (~2 minutes).

For headless or CLI-only setups, run `aletheia init` instead.

## Daily Use

```bash
aletheia start      # start memory services + gateway, open browser
aletheia stop       # stop gateway
aletheia restart    # stop then start
aletheia logs -f    # follow gateway logs
aletheia status     # live metrics (requires running gateway)
aletheia doctor     # validate config and connectivity
```

Run `aletheia help` for the full command reference.

## Memory Infrastructure

`aletheia start` brings up Qdrant and Neo4j automatically if Podman or Docker is installed and `infrastructure/memory/docker-compose.yml` exists.

```
Memory services:
  qdrant      OK running
  neo4j       OK running
  mem0        -  not running (optional)
```

**macOS (native, no Docker):** `brew install qdrant neo4j` - `aletheia start` detects native services.

**Linux / Docker / Podman:** Ensure Docker or Podman is running. `aletheia start` handles the rest.

Skip with `aletheia start --no-memory`. See [DEPLOYMENT.md](DEPLOYMENT.md#memory-sidecar) for Mem0 sidecar setup.

## Optional: Signal Integration

Requires [signal-cli](https://github.com/AsamK/signal-cli) and a registered phone number. See [CONFIGURATION.md](CONFIGURATION.md#channelssignal) for setup.

## Next Steps

- [CONFIGURATION.md](CONFIGURATION.md) - config reference
- [DEPLOYMENT.md](DEPLOYMENT.md) - production setup
- [WORKSPACE_FILES.md](WORKSPACE_FILES.md) - agent workspace files
- [PLUGINS.md](PLUGINS.md) - plugin system
