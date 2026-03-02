# Quick Start

```bash
git clone https://github.com/CKickertz/ergon.git
cd ergon
./setup.sh
```

Your browser opens automatically. If not, visit **http://localhost:18789**.

## What setup.sh does

1. Checks Node.js 22+
2. Builds runtime and UI
3. Creates config at `~/.aletheia/aletheia.json` if needed
4. Starts the gateway on port 18789
5. Opens browser

## Credentials

The setup wizard auto-detects your Anthropic API key from Claude Code's config (`~/.claude.json`). If using OAuth or without Claude Code installed, enter a key manually from `https://console.anthropic.com/keys`.

## After setup

| Task | Command |
|------|---------|
| Start | `aletheia start` |
| Stop | `aletheia stop` |
| Restart | `aletheia restart` |
| Logs | `aletheia logs -f` |
| Status | `aletheia status` |
| Diagnose | `aletheia doctor` |

Run `aletheia help` for the full command reference.

## Memory services

If Podman or Docker is installed, `aletheia start` automatically brings up Qdrant and Neo4j for persistent cross-session memory. Skip with `aletheia start --no-memory`.

## Next steps

- [docs/CONFIGURATION.md](docs/CONFIGURATION.md) — Full config reference
- [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md) — Production setup
- [docs/WORKSPACE_FILES.md](docs/WORKSPACE_FILES.md) — Agent workspace structure
