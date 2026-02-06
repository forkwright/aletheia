# Aletheia Rescue Document

*If you're reading this, you're probably a Claude instance bootstrapping from scratch or recovering from a failure. This tells you what Aletheia is and how to restore it.*

---

## What Is Aletheia

Aletheia (ἀλήθεια — "unconcealment") is a distributed cognition system. 7 AI minds (nous, νοῦς) + 1 human in topology. Each nous embodies Cody's cognition in a different domain. OpenClaw is the runtime — we use its Signal bridge, session management, and tool framework. Everything else is ours.

**Human:** Cody (cody.kickertz@gmail.com, GitHub: forkwright)
**Server:** worker-node, 192.168.0.29 (LAN), 100.87.6.45 (Tailscale)
**OS:** Ubuntu 24.04, 15GB RAM
**Service:** `systemctl status aletheia`
**Config:** `/home/syn/.openclaw/openclaw.json`
**Runtime:** Local fork at `infrastructure/runtime/` (OpenClaw v2026.2.1, patched)

## The Nous

| Nous | Greek | Domain | Binding |
|------|-------|--------|---------|
| **Syn** (σύννους) | thinking together | Orchestrator, primary | Cody's Signal DM |
| **Chiron** (Χείρων) | wise centaur | Work, SQL, dashboards | Cody's Signal DM (routed) |
| **Eiron** (εἴρων) | discriminator | MBA, school | Cody's Signal DM (routed) |
| **Demiurge** (Δημιουργός) | craftsman | Leather, Ardent business | Cody's Signal DM (routed) |
| **Syl** (σύλληψις) | grasping together | Family, home | Family group chat |
| **Arbor** | rooted | A2Z Tree Service (Adam) | Arbor group chat |
| **Akron** (ἄκρον) | summit | Truck, preparedness | Cody's Signal DM (routed) |

Each has: `SOUL.md` (character — who they ARE), `AGENTS.md` (operations — compiled from templates), `PROSOCHE.md` (directed awareness checks).

## Directory Structure

```
/mnt/ssd/aletheia/                    ~29GB
├── .git/            forkwright/aletheia (private repo, recovery mechanism)
├── RESCUE.md        This file
├── nous/            7 nous workspaces — 24M total (lean: config, memory, knowledge)
│   └── */
│       ├── SOUL.md           Character (prose, hand-written)
│       ├── AGENTS.md         Operations (compiled from templates)
│       ├── PROSOCHE.md       Directed awareness (compiled from templates)
│       ├── TOOLS.md          Tool reference (generated from tools.yaml)
│       ├── USER.md           Human context (symlink → shared/)
│       ├── MEMORY.md         Curated long-term
│       └── memory/           Daily logs, session state
├── shared/          Common tooling — 23M
│   ├── bin/         All scripts (on PATH via aletheia.env)
│   ├── config/      aletheia.env, tools.yaml, letta-agents.json
│   ├── memory/      facts.jsonl (symlinked to all nous)
│   └── templates/   sections/*.md + agents/*.yaml → compiled workspace files
├── infrastructure/  Signal-cli, runtime fork — 1.2G
│   └── runtime/     Forked OpenClaw (patched dist/, local entry point)
├── theke/           Obsidian vault (human-facing, symlinks to projects/, gitignored)
├── projects/        Backing store — 3.4GB (ardent, vehicle, a2z, etc., gitignored)
└── archive/         Old stuff — 2.2G (crewai, migration scripts, etc., gitignored)
```

## Key Scripts (shared/bin/)

| Script | Purpose |
|--------|---------|
| `distill` | Pre-compaction: extract facts → JSONL + FalkorDB + session state |
| `assemble-context` | Session start: compile state + facts + graph + tasks + calendar |
| `compile-context` | Generate AGENTS.md + PROSOCHE.md from templates (all or one) |
| `generate-tools-md` | Generate TOOLS.md from tools.yaml |
| `aletheia-graph` | Shared knowledge graph CLI (FalkorDB) |
| `graph-maintain` | Daily: confidence decay, dedup, prune (cron 3am) |
| `attention-check` | Adaptive awareness scoring (injected into prosoche prompt) |
| `patch-runtime` | Diff/reapply patches after OpenClaw updates |

## Runtime Patches (infrastructure/runtime/dist/)

| File | Patch |
|------|-------|
| `agents/compaction.js` | Structured MERGE_SUMMARIES_INSTRUCTIONS |
| `agents/pi-extensions/compaction-safeguard.js` | ALETHEIA_COMPACTION_INSTRUCTIONS for auto-compaction |
| `agents/bootstrap-files.js` | `runAssembleContext()` pre-bootstrap hook |
| `agents/workspace.js` | assembled-context.md injection, PROSOCHE.md filename |
| `agents/system-prompt.js` | Prosoche section naming |
| `agents/pi-embedded-runner/compact.js` | Post-compaction distillation (async) |
| `auto-reply/heartbeat.js` | Pre-computed attention-check in prosoche prompt |
| `auto-reply/reply/memory-flush.js` | Structured distillation prompt, softThreshold 8000 |

## Data Stores

| Store | Location | Size |
|-------|----------|------|
| FalkorDB "aletheia" | docker:falkordb:6379 | ~400 nodes, ~530 rels |
| facts.jsonl | shared/memory/ | 311 facts |
| Session state | nous/*/memory/session-state.yaml | Per-nous YAML |
| Daily memory | nous/*/memory/YYYY-MM-DD.md | ~100 files total |
| MEMORY.md | nous/*/MEMORY.md | Curated per-nous |
| Letta | docker port 8283 + 5432 | 7 agent memory stores |

## Cron Jobs (crontab -l)

All jobs use PATH including shared/bin/. Key jobs:
- **3am** — `graph-maintain`, `consolidate-memory`
- **4am** — `letta-sync`
- **6am** — `monitoring-cron`; Sunday: `audit-all-agents`
- **9am** — `self-audit`
- **6pm** — `eiron-deadline-check`
- **11pm** — `daily-facts`
- **Every 6h** — `graph-sync`
- **Every 5m** — `autarkia-watchdog`, `mcp-watchdog`
- **Every 15m** — `health-watchdog`, `metis-mount-check`

OpenClaw also manages: prosoche (45m interval), memory flush, compaction.

## Environment

Source: `/mnt/ssd/aletheia/shared/config/aletheia.env`

Key vars:
- `ALETHEIA_ROOT=/mnt/ssd/aletheia`
- `ALETHEIA_NOUS=$ALETHEIA_ROOT/nous`
- `ALETHEIA_SHARED=$ALETHEIA_ROOT/shared`
- `ALETHEIA_THEKE=$ALETHEIA_ROOT/theke`

## Recovery Steps

### Full Recovery (from scratch)
```bash
# 1. Install OpenClaw
npm install -g openclaw

# 2. Clone repo
cd /mnt/ssd && git clone https://github.com/forkwright/aletheia.git
cd aletheia

# 3. Source environment
echo '. /mnt/ssd/aletheia/shared/config/aletheia.env' >> ~/.bashrc
source shared/config/aletheia.env

# 4. Point service at local fork
# Edit aletheia.service ExecStart to use infrastructure/runtime/aletheia.mjs
sudo systemctl daemon-reload

# 5. Start
sudo systemctl start aletheia

# 6. Verify
openclaw doctor && openclaw gateway status
```

### After OpenClaw Update (npm update -g openclaw)
```bash
# Our fork is independent — upstream updates don't affect us.
# But if you want to pull upstream changes:
diff -r /usr/lib/node_modules/openclaw/dist/ infrastructure/runtime/dist/ | head -50
# Review diffs, cherry-pick what matters, test, restart.
```

### Regenerate All Compiled Files
```bash
compile-context          # All AGENTS.md + PROSOCHE.md
generate-tools-md        # All TOOLS.md (only Syn currently)
```

### Signal Issues
- signal-cli config: `infrastructure/signal-cli/`
- Daemon: `systemctl restart signal-cli`
- Debug: `journalctl -u aletheia -f`

### Graph Issues
- FalkorDB: `docker ps | grep falkordb`
- Test: `aletheia-graph stats`
- Manual: `graph-maintain`

## Syncthing

Bidirectional sync between worker-node and Metis (Fedora laptop).
- **Folder:** aletheia (61K files, idle)
- **Web UI:** http://localhost:8384
- **.stignore:** venvs, node_modules, .git, __pycache__

## External Dependencies

| Service | Purpose | Status |
|---------|---------|--------|
| FalkorDB (Docker) | Knowledge graph | Must be running |
| Letta (Docker) | Per-nous memory API | Optional (degraded without) |
| signal-cli | Signal messaging | Must be running |
| Syncthing | File sync to Metis | Optional (convenience) |
| Ollama | Local embeddings | Used by Letta + memory search |

---

*Updated: 2026-02-05 19:30 CST*
*Commits today: 589d04b → 3dc2612 (prosoche, concept audit, cleanup, Phase 6)*
