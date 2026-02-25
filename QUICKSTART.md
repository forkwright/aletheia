# Quick Start

```bash
git clone https://github.com/forkwright/aletheia
cd aletheia
./setup.sh        # builds, installs CLI, opens browser
aletheia start    # from next time on
```

Your browser will open automatically. Follow the setup wizard — it takes about two minutes.

If the browser doesn't open, visit: **http://localhost:18789**

## What setup.sh does

1. Checks Node.js 20+ is present
2. Builds the runtime and UI
3. Creates a minimal config at `~/.aletheia/aletheia.json` if one doesn't exist
4. Starts the gateway on port 18789
5. Opens your browser

## Credential detection

The wizard will attempt to auto-detect your Anthropic API key from Claude Code's config (`~/.claude.json`). If you use Claude Code via OAuth (browser login), or don't have Claude Code installed, enter an API key manually — get one at `https://console.anthropic.com/keys`.

## After setup

| Task | Command |
|------|---------|
| Start | `aletheia start` |
| Stop | `aletheia stop` |
| Restart | `aletheia restart` |
| View logs | `aletheia logs -f` |
| Health check | `aletheia status` |
| Diagnose issues | `aletheia doctor` |

Run `aletheia help` for the full command reference.


## Memory services

If you have Podman or Docker installed and `infrastructure/memory/docker-compose.yml` is present,
`aletheia start` will automatically bring up Qdrant and Neo4j for persistent cross-session memory.
Skip with `aletheia start --no-memory`.

## Advanced configuration

See `docs/` for full configuration reference, multi-agent setup, and deployment guides.
