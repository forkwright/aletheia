# Quick Start

```bash
git clone https://github.com/forkwright/aletheia
cd aletheia
./setup.sh
```

Your browser will open automatically. Follow the setup wizard — it takes about two minutes.

## What setup.sh does

1. Checks Node.js 20+ is present
2. Builds the runtime and UI
3. Creates a minimal config at `~/.aletheia/aletheia.json` if one doesn't exist
4. Starts the gateway on port 18789
5. Opens your browser

## Credential detection

The wizard will attempt to auto-detect your Anthropic API key from Claude Code's config (`~/.claude.json`). If you don't have Claude Code installed, enter an API key manually at `https://console.anthropic.com/keys`.

## After setup

Your agent's workspace lives at `<repo>/nous/<agent-id>/`. It starts with an onboarding SOUL.md — your first conversation calibrates the agent to your domain and style.

## Manual start (after first run)

```bash
ALETHEIA_ROOT=$(pwd) node infrastructure/runtime/dist/entry.mjs
```

## Advanced configuration

See `docs/` for full configuration reference, multi-agent setup, and deployment guides.
