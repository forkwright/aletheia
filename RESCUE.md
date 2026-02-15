# Aletheia Rescue Document

*Recovery guide for restoring the Aletheia system from scratch.*

---

## What Is Aletheia

Aletheia (ἀλήθεια — "unconcealment") is a distributed cognition system. Multiple AI minds (nous, νοῦς) + 1 human in topology. Each nous embodies a domain of the operator's cognition.

**Service:** `systemctl status aletheia`
**Config:** `~/.aletheia/aletheia.json`
**Runtime:** `infrastructure/runtime/` (compiled with tsdown)

## Directory Structure

```
aletheia/
├── .git/                 Source control (recovery mechanism)
├── nous/                 Agent workspaces
│   ├── _example/             Template (start here)
│   └── {agent}/
│       ├── SOUL.md           Character (prose, hand-written)
│       ├── USER.md           Human operator context
│       ├── AGENTS.md         Operations guide
│       ├── MEMORY.md         Curated long-term
│       ├── GOALS.md          Goal hierarchy
│       ├── IDENTITY.md       Name, emoji
│       ├── PROSOCHE.md       Directed awareness
│       ├── TOOLS.md          Tool reference (generated)
│       ├── CONTEXT.md        Session-scoped dynamic
│       └── memory/           Daily logs, session state
├── shared/               Common tooling
│   ├── bin/              Scripts (on PATH via aletheia.env)
│   ├── config/           aletheia.env, tools.yaml
│   └── templates/        sections/*.md + agents/*.yaml → compiled workspace files
├── infrastructure/       Runtime, memory, signal-cli
│   ├── runtime/          Gateway (TypeScript, tsdown)
│   ├── memory/           Mem0 sidecar + Qdrant + Neo4j (Docker)
│   └── prosoche/         Adaptive attention daemon
└── config/               Example configs
```

## Runtime Architecture (infrastructure/runtime/)

| Module | Purpose |
|--------|---------|
| `taxis/` | Config loading + Zod validation |
| `nous/` | Agent lifecycle, bootstrap, system prompt |
| `mneme/` | SQLite session store (WAL mode) |
| `hermeneus/` | Anthropic SDK, model routing |
| `organon/` | Tool framework + built-in tools |
| `semeion/` | Signal channel (daemon, client, listener, sender) |
| `pylon/` | Hono HTTP gateway with auth |
| `distillation/` | Context distillation pipeline |
| `prostheke/` | Plugin system with lifecycle hooks |
| `daemon/` | Process lifecycle, cron scheduler |
| `koina/` | Shared utilities (logger, crypto, fs, errors) |

## Data Stores

| Store | Location | Purpose |
|-------|----------|---------|
| Mem0 (Qdrant) | docker:qdrant:6333 | Vector memories (auto-extracted) |
| Mem0 (Neo4j) | docker:neo4j:7687 | Entity graph (auto-extracted) |
| Mem0 sidecar | systemd:aletheia-memory:8230 | FastAPI extraction engine |
| Sessions | `~/.aletheia/sessions.db` | SQLite session store |
| MEMORY.md | `nous/*/MEMORY.md` | Curated per-agent |

## External Dependencies

| Service | Purpose | Required? |
|---------|---------|-----------|
| Qdrant (Docker) | Vector store for Mem0 | Yes |
| Neo4j (Docker) | Graph store for Mem0 | Yes |
| Mem0 sidecar (systemd) | Memory extraction | Yes |
| signal-cli (Docker or native) | Signal messaging | Yes |
| Langfuse (Docker) | Observability | No (monitoring) |

## Recovery Steps

### Full Recovery (from scratch)

```bash
# 1. Clone repo
git clone https://github.com/forkwright/aletheia.git
cd aletheia

# 2. Create environment file
cp .env.example shared/config/aletheia.env
# Edit aletheia.env — fill in API keys and paths

# 3. Install runtime deps
cd infrastructure/runtime && npm install && npx tsdown && cd ../..

# 4. Start memory infrastructure
cd infrastructure/memory && docker compose up -d  # Qdrant + Neo4j
cd sidecar && uv venv && source .venv/bin/activate && uv pip install -e .
sudo cp aletheia-memory.service /etc/systemd/system/
sudo systemctl enable --now aletheia-memory

# 5. Create gateway config
mkdir -p ~/.aletheia/credentials
cp config/aletheia.example.json ~/.aletheia/aletheia.json
# Edit aletheia.json — configure agents, bindings, Signal number

# 6. Create your first agent workspace
cp -r nous/_example nous/your-agent-name
# Edit SOUL.md, USER.md, IDENTITY.md to customize

# 7. Start main service
sudo cp aletheia.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now aletheia

# 8. Verify
systemctl status aletheia aletheia-memory
curl -s http://localhost:8230/health
curl -s http://localhost:6333/healthz
curl -s http://localhost:18789/health
```

### Regenerate Compiled Files

```bash
compile-context          # All AGENTS.md + PROSOCHE.md
generate-tools-md        # All TOOLS.md
```

### Signal Issues

- signal-cli config: `infrastructure/signal-cli/`
- Restart: `systemctl restart signal-cli` or `docker restart signal-cli`
- Debug: `journalctl -u aletheia -f`

### Memory Issues

- Mem0 sidecar: `systemctl status aletheia-memory`
- Qdrant: `curl -s http://localhost:6333/healthz`
- Neo4j: `curl -s http://localhost:7474`
- Search test: `curl -s -X POST http://localhost:8230/search -H 'Content-Type: application/json' -d '{"query":"test","user_id":"default","limit":5}'`

---

*See `README.md` for full architecture. See `ALETHEIA.md` for philosophy.*
