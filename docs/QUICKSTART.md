# Quickstart

## Prerequisites

- Node.js >= 22.12
- Docker and Docker Compose
- signal-cli ([install guide](https://github.com/AsamK/signal-cli))
- A registered Signal phone number

## 1. Build

```bash
git clone https://github.com/forkwright/aletheia.git && cd aletheia
cd infrastructure/runtime && npm install && npx tsdown && cd ../..
```

## 2. Configure

```bash
cp .env.example shared/config/aletheia.env
# Edit: ANTHROPIC_API_KEY, ALETHEIA_ROOT

mkdir -p ~/.aletheia/credentials
cp config/aletheia.example.json ~/.aletheia/aletheia.json
# Edit: agent workspace paths, Signal number, bindings
```

See [CONFIGURATION.md](CONFIGURATION.md) for full reference.

## 3. Memory Infrastructure (optional)

```bash
cd infrastructure/memory && docker compose up -d    # Qdrant + Neo4j
```

For the Mem0 sidecar, see [DEPLOYMENT.md](DEPLOYMENT.md#memory-sidecar).

## 4. Create an Agent

```bash
cp -r nous/_example nous/atlas
# Edit: SOUL.md (identity), USER.md (operator), IDENTITY.md (name + emoji)
```

See [WORKSPACE_FILES.md](WORKSPACE_FILES.md) for file reference.

## 5. Build Web UI (optional)

```bash
cd ui && npm install && npm run build && cd ..
```

## 6. Run

```bash
node infrastructure/runtime/aletheia.mjs gateway
```

## 7. Verify

```bash
curl http://localhost:18789/health       # Gateway
open http://localhost:18789/ui           # Web UI
aletheia send -a atlas -m "Hello"       # Test message
```

Signal: send a message to the registered number. Use `!help` for commands.

## Next Steps

- [CONFIGURATION.md](CONFIGURATION.md) — full config reference
- [DEPLOYMENT.md](DEPLOYMENT.md) — production setup with systemd
- [WORKSPACE_FILES.md](WORKSPACE_FILES.md) — agent workspace files
- [PLUGINS.md](PLUGINS.md) — plugin system
