# Quickstart

Zero to running in 5 minutes.

## Prerequisites

- Node.js >= 22.12
- Docker and Docker Compose
- signal-cli ([install guide](https://github.com/AsamK/signal-cli))
- A registered Signal phone number

## 1. Clone and Build

```bash
git clone https://github.com/forkwright/aletheia.git
cd aletheia

cd infrastructure/runtime
npm install
npx tsdown
cd ../..
```

## 2. Configure Environment

```bash
cp .env.example shared/config/aletheia.env
```

Edit `shared/config/aletheia.env` — at minimum set:
- `ANTHROPIC_API_KEY` — your Anthropic API key
- `ALETHEIA_ROOT` — absolute path to this repo

## 3. Start Memory Infrastructure

```bash
cd infrastructure/memory
docker compose up -d    # Qdrant + Neo4j
```

Optional: set up the Mem0 sidecar for automatic memory extraction (see [DEPLOYMENT.md](DEPLOYMENT.md)).

## 4. Create Gateway Config

```bash
mkdir -p ~/.aletheia/credentials
cp config/aletheia.example.json ~/.aletheia/aletheia.json
```

Edit `~/.aletheia/aletheia.json`:
- Set agent workspace paths to absolute paths
- Set your Signal phone number in `channels.signal.accounts.default.account`
- Set Signal UUIDs in bindings
- Generate a random gateway auth token

See [CONFIGURATION.md](CONFIGURATION.md) for full reference.

## 5. Create Your First Agent

```bash
cp -r nous/_example nous/atlas
```

Edit the workspace files:
- `SOUL.md` — define who this agent is (character, voice, values)
- `USER.md` — describe yourself as the operator
- `IDENTITY.md` — set name and emoji

See [WORKSPACE_FILES.md](WORKSPACE_FILES.md) for details on each file.

## 6. Run

```bash
# Direct:
node infrastructure/runtime/aletheia.mjs gateway

# Or install globally:
sudo ln -s $(pwd)/infrastructure/runtime/aletheia.mjs /usr/local/bin/aletheia
aletheia gateway
```

## 7. Verify

```bash
# Health check
curl http://localhost:18789/health

# System status
curl http://localhost:18789/api/status

# Send a test message via CLI
aletheia send -a atlas -m "Hello, are you there?"
```

## 8. Signal Integration

Once signal-cli is running (JSON-RPC on port 8080), send a message to the registered Signal number. The gateway routes it to the default agent.

Use `!help` in Signal to see available commands.

## Next Steps

- [CONFIGURATION.md](CONFIGURATION.md) — full config reference
- [WORKSPACE_FILES.md](WORKSPACE_FILES.md) — agent workspace file guide
- [DEPLOYMENT.md](DEPLOYMENT.md) — production setup with systemd
- [PLUGINS.md](PLUGINS.md) — plugin system and memory plugin
