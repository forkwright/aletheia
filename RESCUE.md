# Aletheia Rescue Document

*If you're reading this, you're probably a Claude instance bootstrapping from scratch or recovering from a failure. This tells you what Aletheia is and how to restore it.*

---

## What Is Aletheia

Aletheia (ἀλήθεια — "unconcealment") is a distributed cognition system. 7 AI minds (nous, νοῦς) + 1 human in topology. Each nous embodies the operator's cognition in a different domain. OpenClaw is the runtime — we use its Signal bridge, session management, and tool framework. Everything else is ours.

**Human:** the operator (user@example.com, GitHub: forkwright)
**Server:** server-host, SERVER_IP (LAN), TAILSCALE_SERVER (Tailscale)
**OS:** Ubuntu 24.04
**Service:** `systemctl status aletheia` (was: moltbot → autarkia → aletheia)
**Config:** `/home/syn/.openclaw/openclaw.json`
**Runtime:** OpenClaw (MIT, npm package) — patched in-place

## The Nous (Agents)

| Nous | Domain | Binding |
|------|--------|---------|
| **Syn** | Orchestrator, primary | the operator's Signal DM |
| **Chiron** | Work, SQL, data, dashboards | the operator's Signal DM (routed by Syn) |
| **Eiron** | MBA, school, academics | the operator's Signal DM (routed by Syn) |
| **Demiurge** | Leather, craft, Ardent business | the operator's Signal DM (routed by Syn) |
| **Syl** | Partner, family, home | Family group chat |
| **Arbor** | Example Tree Service (Adam) | Adam's Signal DM |
| **Akron** | Truck, Cummins, preparedness | the operator's Signal DM (routed by Syn) |

## Directory Structure

```
/mnt/ssd/aletheia/                    ~34GB
├── nous/            7 agent workspaces (machine-facing)
├── shared/          Common tooling, templates, config
│   ├── bin/         All custom scripts (on PATH)
│   ├── config/      aletheia.env, tools.yaml
│   ├── memory/      facts.jsonl (symlinked to all agents)
│   └── templates/   Shared sections + per-agent YAML
├── infrastructure/  Patches, signal-cli, repos
├── theke/           Obsidian vault (human-facing, gitignored)
├── projects/        Backing store for theke (gitignored)
└── archive/         Old stuff
```

## Key Scripts (shared/bin/)

| Script | Purpose |
|--------|---------|
| `distill` | Pre-compaction: extract facts → JSONL + FalkorDB + session state |
| `assemble-context` | Session start: compile state + facts + graph + tasks + calendar |
| `compile-context` | Generate AGENTS.md from templates |
| `compile-full-context` | Generate single CONTEXT.md per agent |
| `generate-tools-md` | Generate TOOLS.md from tools.yaml |
| `aletheia-graph` | Shared knowledge graph CLI (FalkorDB) |
| `graph-maintain` | Daily hygiene: confidence decay, dedup, prune |
| `attention-check` | Adaptive awareness (replaces heartbeat) |
| `patch-runtime` | Reapply patches after OpenClaw updates |

## Data Stores

| Store | Location | Purpose |
|-------|----------|---------|
| FalkorDB "aletheia" | docker:falkordb:6379 | Shared knowledge graph (~400 nodes) |
| facts.jsonl | shared/memory/ | 311 structured facts |
| Session state | nous/*/memory/session-state.yaml | Per-nous continuity |
| Daily memory | nous/*/memory/YYYY-MM-DD.md | Session logs |
| MEMORY.md | nous/*/MEMORY.md | Curated long-term per-nous |

## OpenClaw Patches

Two patches applied to `/usr/lib/node_modules/openclaw/dist/`:

1. **Signal group ID** — Removed `.toLowerCase()` on base64 group IDs in `channels/plugins/normalize/signal.js`
2. **Dynamic context** — `agents/workspace.js` loads CONTEXT.md instead of 7 files when present

Reapply after updates: `patch-runtime`
Backup: `workspace.js.aletheia-backup`

## Recovery Steps

### Full Recovery (from scratch)
1. Install OpenClaw: `npm install -g openclaw`
2. Clone this repo to `/mnt/ssd/aletheia/`
3. Source env: `. /mnt/ssd/aletheia/shared/config/aletheia.env`
4. Ensure PATH includes `shared/bin/`
5. Apply patches: `patch-runtime`
6. Start service: `systemctl start aletheia`
7. Verify: `openclaw doctor && openclaw gateway status`

### After OpenClaw Update
1. `patch-runtime`
2. `compile-context` (regenerate all AGENTS.md)
3. `generate-tools-md` (regenerate all TOOLS.md)
4. `compile-full-context` (regenerate all CONTEXT.md)
5. `openclaw gateway restart`

### Signal Issues
- signal-cli config: `/mnt/ssd/aletheia/infrastructure/signal-cli/`
- If daemon dies: `systemctl restart signal-cli`
- If messages not routing: check `journalctl -u aletheia -f`

### Graph Issues
- FalkorDB in Docker: `docker ps | grep falkordb`
- Test: `aletheia-graph stats`
- Manual maintenance: `graph-maintain`

## Cron Jobs
- **3:00 AM daily** — `graph-maintain` (confidence decay, dedup, prune)
- **Heartbeats** — OpenClaw-managed, calls `attention-check`

## Environment Variables
Source: `/mnt/ssd/aletheia/shared/config/aletheia.env`
Key vars: `ALETHEIA_ROOT`, `ALETHEIA_NOUS`, `ALETHEIA_SHARED`, `ALETHEIA_THEKE`

---

*Updated: 2026-02-05*
*This document lives in the repo and is updated as the system evolves.*
