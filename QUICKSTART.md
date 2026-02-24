# Quick Start

```bash
git clone https://github.com/forkwright/aletheia
cd aletheia
./setup.sh        # builds, installs CLI, opens browser
aletheia start    # from next time on
```

The browser opens automatically. Follow the setup wizard — it takes about two minutes.

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

## Credential detection

The wizard auto-detects your Anthropic API key from Claude Code's config (`~/.claude.json`).
If you don't have Claude Code installed, enter a key manually at https://console.anthropic.com/keys.

> **OAuth users:** Claude Code's OAuth session doesn't include an API key. Get a separate key at the link above.

## Memory services

If you have Podman or Docker installed and `infrastructure/memory/docker-compose.yml` is present,
`aletheia start` will automatically bring up Qdrant and Neo4j for persistent cross-session memory.
Skip with `aletheia start --no-memory`.

## Advanced configuration

See `docs/` for full configuration reference, multi-agent setup, and deployment guides.
