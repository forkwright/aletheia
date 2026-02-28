# Instance Scaffold

This directory defines the structure for an Aletheia deployment. To initialize:

```bash
aletheia init
# or manually:
cp -r instance.example instance
```

Then configure `instance/config/aletheia.yaml` and add your agents under `instance/nous/`.

## Directory Structure

```
instance/
├── theke/          # Human + nous collaborative space (shared knowledge, research, projects)
├── shared/         # Nous-only shared resources (tools, skills, hooks, templates)
├── nous/           # Individual agent workspaces
│   └── _template/  # Template for new agents (copied by `aletheia add-nous`)
├── config/         # Deployment configuration (YAML, credentials)
├── data/           # Runtime data stores (SQLite, planning DB)
├── signal/         # Signal-cli data directory
└── logs/           # Runtime logs
```

## Three-Tier Cascade

Resolution order (most specific wins):
1. `instance/nous/{id}/` — agent-specific overrides
2. `instance/shared/` — shared across all agents
3. `instance/theke/` — human + agent collaborative space

Tools, templates, hooks, and config all resolve through this cascade.

## What Goes Where

| Content | Location | Why |
|---------|----------|-----|
| Agent identity (SOUL.md, TELOS.md) | `nous/{id}/` | Per-agent |
| Agent-specific tools | `nous/{id}/tools/` | Only this agent sees them |
| Shared tools | `shared/tools/` | All agents see them |
| Human-facing tools | `theke/tools/` | Human + agents |
| Research, deliberations | `theke/` | Collaborative work products |
| API keys, OAuth tokens | `config/credentials/` | Deployment secrets |
| Session database | `data/sessions.db` | Runtime state |

See `docs/specs/44_oikos.md` for the full design rationale.
