# Quickstart

## Prerequisites

- Node.js >= 20
- An Anthropic API key (or Claude Code installed — key is auto-detected)

## Install

```bash
git clone https://github.com/forkwright/aletheia.git
cd aletheia
./setup.sh
```

`setup.sh` builds the runtime and UI, installs the `aletheia` CLI to `~/.local/bin`, starts
the gateway, and opens your browser. Follow the setup wizard (~2 minutes).

## Daily use

```bash
aletheia start      # start memory services + gateway, open browser
aletheia stop       # stop gateway
aletheia restart    # stop then start
aletheia logs -f    # follow gateway logs
aletheia status     # health check
aletheia doctor     # validate config and connectivity
```

Run `aletheia help` for the full command reference.

## Memory infrastructure

`aletheia start` automatically brings up Qdrant and Neo4j if Podman or Docker is installed
and `infrastructure/memory/docker-compose.yml` is present. Status is shown on startup:

```
Memory services:
  qdrant      ✓ running
  neo4j       ✓ running
  mem0        - not running (optional)
```

Skip with `aletheia start --no-memory`. See [DEPLOYMENT.md](DEPLOYMENT.md#memory-sidecar)
for Mem0 sidecar setup.

## Optional: Signal integration

Requires [signal-cli](https://github.com/AsamK/signal-cli) and a registered phone number.
See [CONFIGURATION.md](CONFIGURATION.md#signal) for setup.

## Next steps

- [CONFIGURATION.md](CONFIGURATION.md) — full config reference
- [DEPLOYMENT.md](DEPLOYMENT.md) — production setup
- [WORKSPACE_FILES.md](WORKSPACE_FILES.md) — agent workspace files
- [PLUGINS.md](PLUGINS.md) — plugin system
