# Quickstart

## Prerequisites

- Node.js >= 22.12
- An Anthropic API key

## 1. Clone and Build

```bash
git clone https://github.com/forkwright/aletheia.git && cd aletheia
cd infrastructure/runtime && npm install && npx tsdown && cd ../..
cd ui && npm install && npm run build && cd ..
```

## 2. Initialize

```bash
npx infrastructure/runtime/aletheia.mjs init
```

The wizard will prompt for:
- Anthropic API key
- Gateway port (default: 18789)
- Auth mode (default: none for local use)
- Aletheia root directory
- First agent name and emoji

## 3. Start

```bash
npx infrastructure/runtime/aletheia.mjs gateway start
```

Or if installed globally:

```bash
aletheia gateway start
```

## 4. Open the UI

```
http://localhost:18789/ui
```

Your first agent will guide you through onboarding — it asks about your preferences, communication style, and what you need help with, then writes its own identity files.

## 5. Verify

```bash
aletheia doctor                         # Validate config
aletheia status                         # Check running gateway
curl http://localhost:18789/health      # Health check
```

## Optional: Memory Infrastructure

For long-term memory with vector search and knowledge graphs:

```bash
cd infrastructure/memory && docker compose up -d    # Qdrant + Neo4j
```

See [DEPLOYMENT.md](DEPLOYMENT.md#memory-sidecar) for the Mem0 sidecar setup.

## Optional: Signal Integration

Requires [signal-cli](https://github.com/AsamK/signal-cli) and a registered phone number. See [CONFIGURATION.md](CONFIGURATION.md#signal) for setup.

## Next Steps

- [CONFIGURATION.md](CONFIGURATION.md) — full config reference
- [DEPLOYMENT.md](DEPLOYMENT.md) — production setup with systemd
- [WORKSPACE_FILES.md](WORKSPACE_FILES.md) — agent workspace files
- [PLUGINS.md](PLUGINS.md) — plugin system
