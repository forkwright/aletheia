# Aletheia Rescue Document

*Recovery guide for restoring the Aletheia system from scratch.*

---

## What Is Aletheia

Aletheia is a distributed cognition system. Multiple AI minds (nous, νοῦς) + 1 human in topology. Each nous embodies a domain of the operator's cognition.

**Service:** `systemctl status aletheia`
**Config:** `~/.aletheia/aletheia.json`
**Runtime:** `infrastructure/runtime/` (compiled with tsdown, ~354KB)
**Web UI:** `/ui` (Svelte 5, built from `ui/`)

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
├── ui/                   Web UI (Svelte 5 + TypeScript)
│   └── dist/             Build output (served at /ui)
├── infrastructure/       Runtime, memory, signal-cli
│   ├── runtime/          Gateway (TypeScript, tsdown)
│   ├── memory/           Mem0 sidecar + Qdrant + Neo4j (Docker)
│   └── prosoche/         Adaptive attention daemon
└── config/               Example configs
```

## Runtime Modules

| Module | Purpose |
|--------|---------|
| `taxis/` | Config loading + Zod validation |
| `nous/` | Agent lifecycle, bootstrap, turn execution |
| `mneme/` | SQLite session store (WAL mode, 6 migrations) |
| `hermeneus/` | Anthropic SDK, provider router, complexity routing |
| `organon/` | Tool registry + 28 built-in tools + skills |
| `semeion/` | Signal channel (client, listener, sender, commands) |
| `pylon/` | Hono HTTP gateway, MCP, Web UI |
| `distillation/` | Context summarization pipeline |
| `prostheke/` | Plugin system with lifecycle hooks |
| `daemon/` | Cron scheduler, watchdog health probes |
| `koina/` | Shared utilities (logger, crypto, event bus) |

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
| Langfuse (Docker) | Observability | No |

## Recovery Steps

### Full Recovery (from scratch)

```bash
# 1. Clone repo
git clone https://github.com/forkwright/aletheia.git
cd aletheia

# 2. Create environment file
cp .env.example shared/config/aletheia.env
# Edit aletheia.env — fill in API keys and paths
# MUST be systemd EnvironmentFile compatible (no `export`, no variable refs)

# 3. Build runtime
cd infrastructure/runtime && npm install && npx tsdown && cd ../..

# 4. Build Web UI
cd ui && npm install && npm run build && cd ..

# 5. Start memory infrastructure
cd infrastructure/memory && docker compose up -d  # Qdrant + Neo4j
cd sidecar && uv venv && source .venv/bin/activate && uv pip install -e .
sudo cp aletheia-memory.service /etc/systemd/system/
sudo systemctl enable --now aletheia-memory

# 6. Create gateway config
mkdir -p ~/.aletheia/credentials
cp config/aletheia.example.json ~/.aletheia/aletheia.json
# Edit aletheia.json — configure agents, bindings, Signal number

# 7. Create your first agent workspace
cp -r nous/_example nous/your-agent-name
# Edit SOUL.md, USER.md, IDENTITY.md to customize

# 8. Start main service
sudo cp aletheia.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now aletheia

# 9. Verify
systemctl status aletheia aletheia-memory
curl -s http://localhost:8230/health
curl -s http://localhost:6333/healthz
curl -s http://localhost:18789/health
# Web UI: http://localhost:18789/ui
```

### Post-Clone Checklist

These files are gitignored and must be restored manually after a fresh clone:
- `shared/config/aletheia.env` — environment variables
- `~/.aletheia/aletheia.json` — gateway config
- `~/.aletheia/credentials/` — API keys
- `infrastructure/memory/sidecar/.venv/` — Python venv

### File Permission Notes

- `aletheia.mjs` must be executable: `git update-index --chmod=+x infrastructure/runtime/aletheia.mjs`
- Service user needs ACL access to shared/bin: `setfacl -m u:<user>:rwx shared/bin/*`

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

*See `README.md` for full architecture. See `docs/` for detailed guides.*
