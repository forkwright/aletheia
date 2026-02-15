# Aletheia Rescue Document

*If you're reading this, you're probably a Claude instance bootstrapping from scratch or recovering from a failure. This tells you what Aletheia is and how to restore it.*

---

## What Is Aletheia

Aletheia (ἀλήθεια — "unconcealment") is a distributed cognition system. 6 AI minds (nous, νοῦς) + 1 human in topology. Each nous embodies the operator's cognition in a different domain.

**Human:** the operator (user@example.com, GitHub: forkwright)
**Server:** server-host, SERVER_IP (LAN), TAILSCALE_SERVER (Tailscale)
**OS:** Fedora Server, 15GB RAM
**Service:** `systemctl status aletheia`
**Config:** `/home/syn/.openclaw/aletheia.json`
**Runtime:** Clean-room at `infrastructure/runtime/` (compiled with tsdown)

## The Nous

| Nous | Greek | Domain | Binding |
|------|-------|--------|---------|
| **Syn** (σύννους) | thinking together | Orchestrator, primary | the operator's Signal DM |
| **Eiron** (εἴρων) | discriminator | MBA, school | the operator's Signal DM (routed) |
| **Demiurge** (Δημιουργός) | craftsman | Leather, Ardent business | the operator's Signal DM (routed) |
| **Syl** (σύλληψις) | grasping together | Family, home | Family group chat |
| **Arbor** | rooted | Example Tree Service (Adam) | Arbor group chat |
| **Akron** (ἄκρον) | summit | Truck, preparedness | the operator's Signal DM (routed) |

Each has: `SOUL.md` (character — who they ARE), `AGENTS.md` (operations — compiled from templates), `PROSOCHE.md` (directed awareness checks).

## Directory Structure

```
/mnt/ssd/aletheia/                    ~29GB
├── .git/            forkwright/aletheia (private repo, recovery mechanism)
├── RESCUE.md        This file
├── README.md        Repo overview
├── nous/            6 nous workspaces
│   └── */
│       ├── SOUL.md           Character (prose, hand-written)
│       ├── AGENTS.md         Operations (compiled from templates)
│       ├── PROSOCHE.md       Directed awareness (compiled from templates)
│       ├── TOOLS.md          Tool reference (generated from tools.yaml)
│       ├── USER.md           Human context (symlink → shared/)
│       ├── MEMORY.md         Curated long-term
│       └── memory/           Daily logs, session state
├── shared/          Common tooling — 23M
│   ├── bin/         58 scripts (on PATH via aletheia.env)
│   ├── config/      aletheia.env, tools.yaml
│   ├── memory/      facts.jsonl (symlinked to all nous)
│   └── templates/   sections/*.md + agents/*.yaml → compiled workspace files
├── infrastructure/  Signal-cli, runtime, memory — 1.2G
│   └── runtime/     Clean-room Aletheia runtime (TypeScript, tsdown)
├── theke/           Obsidian vault (human-facing, symlinks to projects/, gitignored)
├── projects/        Backing store — 3.4GB (ardent, vehicle, a2z, etc., gitignored)
└── archive/         Old stuff + archived scripts — 2.2G (gitignored)
    └── bin/         21 archived scripts
```

## Key Scripts (shared/bin/)

| Script | Purpose |
|--------|---------|
| `distill` | Pre-compaction: extract facts → JSONL + session state |
| `assemble-context` | Session start: compile state + facts + graph + tasks + calendar |
| `compile-context` | Generate AGENTS.md + PROSOCHE.md from templates (all or one) |
| `generate-tools-md` | Generate TOOLS.md from tools.yaml |
| `aletheia-graph` | Knowledge graph CLI (Neo4j) |
| `graph-maintain` | Daily: confidence decay, dedup, prune (cron 3am) |
| `attention-check` | Adaptive awareness scoring (injected into prosoche prompt) |
| `patch-runtime` | (legacy — no longer needed with clean-room runtime) |
| `nous-health` | Monitor nous ecosystem health |
| `nous-audit` | Audit a nous workspace |
| `bb` | Blackboard coordination between nous |

CLI convention: `--nous` is our flag (e.g. `distill --nous syn`). `--agent` accepted as alias for backward compat.

## Runtime Architecture (infrastructure/runtime/)

Clean-room Aletheia runtime. No OpenClaw dependencies.

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

| Store | Location | Size |
|-------|----------|------|
| Mem0 (Qdrant) | docker:qdrant:6333 | Vector memories (auto-extracted) |
| Mem0 (Neo4j) | docker:neo4j:7687 | Entity graph (auto-extracted) |
| Mem0 sidecar | systemd:aletheia-memory:8230 | FastAPI extraction engine |
| facts.jsonl | shared/memory/ | 384 facts (imported to Mem0) |
| Session state | nous/*/memory/session-state.yaml | Per-nous YAML |
| Daily memory | nous/*/memory/YYYY-MM-DD.md | ~100 files total |
| MEMORY.md | nous/*/MEMORY.md | Curated per-nous |

## Cron Jobs (crontab -l)

All jobs use PATH including shared/bin/. Key jobs:
- **3am** — `graph-maintain`, `consolidate-memory`
- **6am** — `monitoring-cron`; Sunday: `audit-all-nous`
- **9am** — `self-audit`
- **6pm** — `eiron-deadline-check`
- **11pm** — `daily-facts`
- **Every 6h** — `graph-sync`
- **Every 5m** — `aletheia-watchdog`, `mcp-watchdog`
- **Every 15m** — `health-watchdog`, `laptop-mount-check`

The gateway also manages: prosoche (45m interval), memory flush, compaction, Mem0 extraction.

## Environment

Source: `/mnt/ssd/aletheia/shared/config/aletheia.env`

Key vars:
- `ALETHEIA_ROOT=/mnt/ssd/aletheia`
- `ALETHEIA_NOUS=$ALETHEIA_ROOT/nous`
- `ALETHEIA_SHARED=$ALETHEIA_ROOT/shared`
- `ALETHEIA_THEKE=$ALETHEIA_ROOT/theke`

## Syncthing

Two sync folders between server-host and Laptop (Fedora laptop):

| Folder | Path (server) | Path (Laptop) | Direction |
|--------|---------------|--------------|-----------|
| `aletheia` | `/mnt/ssd/aletheia` | `/home/ck/aletheia` | Server → Laptop (sendonly/receiveonly) |
| `aletheia-vault` | `/mnt/ssd/aletheia/theke` | `/home/ck/aletheia/theke` | Bidirectional (sendreceive) |

`.stignore` excludes: archive/, projects/, infrastructure/, theke/ (separate folder), .git, venvs, node_modules, sync-conflict files.

Laptop has symlink: `/home/syn/aletheia` → `/home/ck/aletheia`. Ownership: `syn:ck` with setgid.

## External Dependencies

| Service | Purpose | Required? |
|---------|---------|-----------|
| Qdrant (Docker) | Vector store for Mem0 | Yes |
| Neo4j (Docker) | Graph store for Mem0 | Yes |
| Mem0 sidecar (systemd) | Memory extraction | Yes |
| signal-cli (Docker) | Signal messaging | Yes |
| Ollama | Local embeddings (mxbai-embed-large) | Yes |
| Syncthing | File sync to Laptop | No (convenience) |
| Langfuse (Docker) | Observability | No (monitoring) |

## Recovery Steps

### Full Recovery (from scratch)
```bash
# 1. Clone repo
cd /mnt/ssd && git clone https://github.com/forkwright/aletheia.git
cd aletheia

# 2. Install runtime deps
cd infrastructure/runtime && npm install && npx tsdown && cd ../..

# 3. Source environment
echo '. /mnt/ssd/aletheia/shared/config/aletheia.env' >> ~/.bashrc
source shared/config/aletheia.env

# 4. Start memory infrastructure
cd infrastructure/memory && docker compose up -d  # Qdrant + Neo4j
cd sidecar && uv venv && source .venv/bin/activate && uv pip install -e .
sudo cp aletheia-memory.service /etc/systemd/system/
sudo systemctl enable --now aletheia-memory

# 5. Start main service
sudo cp aletheia.service /etc/systemd/system/  # ExecStart → infrastructure/runtime/aletheia.mjs
sudo systemctl daemon-reload
sudo systemctl start aletheia

# 6. Verify
systemctl status aletheia aletheia-memory
curl -s http://localhost:8230/health
curl -s http://localhost:6333/healthz
```

### Regenerate All Compiled Files
```bash
compile-context          # All AGENTS.md + PROSOCHE.md
generate-tools-md        # All TOOLS.md
```

### Signal Issues
- signal-cli config: `infrastructure/signal-cli/`
- Daemon: `systemctl restart signal-cli`
- Debug: `journalctl -u aletheia -f`

### Memory Issues
- Mem0 sidecar: `systemctl status aletheia-memory`
- Qdrant: `curl -s http://localhost:6333/healthz`
- Neo4j: `curl -s http://localhost:7474`
- Search test: `curl -s -X POST http://localhost:8230/search -H 'Content-Type: application/json' -d '{"query":"test","user_id":"ck","limit":5}'`

---

*Updated: 2026-02-13*
