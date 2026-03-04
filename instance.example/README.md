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
├── config/                     # Deployment configuration
│   ├── aletheia.yaml           # Main config (from aletheia.yaml.example)
│   └── credentials/            # API keys, OAuth tokens, Signal creds
│
├── data/                       # Runtime data stores
│   └── *.db                    # SQLite databases (sessions, messages, planning)
│
├── logs/                       # Runtime log output
│
├── nous/                       # Agent workspaces
│   ├── _shared/                # Cross-agent shared workspace
│   │   └── workspace/          # Shared plans DB, references, specs, standards
│   ├── _template/              # Template for new agents (copied by `aletheia add-nous`)
│   │   ├── SOUL.md             # Agent identity and character
│   │   ├── IDENTITY.md         # Name, emoji, avatar
│   │   ├── TELOS.md            # Goals and purpose
│   │   ├── MNEME.md            # Memory configuration
│   │   ├── memory/             # Session logs (memory/YYYY-MM-DD.md)
│   │   ├── workspace/          # Agent-specific working files
│   │   ├── tools/              # Agent-specific tool definitions
│   │   ├── hooks/              # Agent-specific lifecycle hooks
│   │   └── templates/          # Agent-specific templates
│   └── {agent-id}/             # Per-agent workspace (same structure as _template)
│
├── shared/                     # Nous-only shared resources
│   ├── bin/                    # Shared shell scripts and executables
│   ├── calibration/            # Model competence calibration data
│   │   └── competence/         # Per-model competence scores
│   ├── commands/               # Custom slash commands
│   │   └── _examples/          # Example command definitions
│   ├── coordination/           # Cross-agent runtime state
│   │   ├── memory/             # Shared memory store
│   │   ├── prosoche/           # Attention/heartbeat state
│   │   ├── status/             # Agent status files
│   │   └── traces/             # Session trace logs (rotatable)
│   ├── docs/                   # Shared documentation
│   ├── hooks/                  # Shared lifecycle hooks
│   │   ├── _examples/          # Example hook implementations
│   │   └── _templates/         # Hook templates
│   ├── schemas/                # Shared data schemas
│   ├── skills/                 # Learned skills (auto-extracted, NOT in git)
│   ├── templates/              # Shared prompt/doc templates
│   │   ├── agents/             # Agent configuration templates
│   │   └── sections/           # Reusable document sections
│   └── tools/                  # Shared tool definitions
│       └── authored/           # Agent-authored tools
│
├── signal/                     # Signal-cli data directory
│
├── theke/                      # Human + nous collaborative space
│   ├── deliberations/          # Multi-agent deliberation records
│   ├── domains/                # Domain knowledge packs
│   ├── projects/               # Project-specific knowledge
│   ├── research/               # Research artifacts
│   ├── templates/              # Human-facing templates
│   └── tools/                  # Human-facing tool definitions
│
└── ui/                         # Built webchat assets (if deploying UI)
    └── dist/                   # Static build output
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
| Agent session logs | `nous/{id}/memory/` | Per-agent, daily files |
| Agent-specific tools | `nous/{id}/tools/` | Only this agent sees them |
| Cross-agent workspace | `nous/_shared/workspace/` | Planning DB, shared references |
| Shared tools & scripts | `shared/tools/`, `shared/bin/` | All agents see them |
| Learned skills | `shared/skills/` | Auto-extracted, NOT tracked in git |
| Coordination state | `shared/coordination/` | Runtime traces, status, prosoche |
| Human-facing tools | `theke/tools/` | Human + agents |
| Research & deliberations | `theke/` | Collaborative work products |
| API keys, OAuth tokens | `config/credentials/` | Deployment secrets |
| Runtime databases | `data/` | SQLite session/message stores |

## Key Principles

1. **If it doesn't ship to a random GitHub clone, it's instance-only.** Skills, credentials, agent workspaces, traces — all instance.
2. **Agents grow their workspaces organically.** The `_template/` provides the minimum. Agents add subdirectories as needed (research/, drafts/, archive/, etc.).
3. **Coordination state is ephemeral.** Traces and status files can be rotated/purged without data loss.
4. **The planning DB can grow large.** `nous/_shared/workspace/plans.db` accumulates messages, sessions, and tool stats. Plan for periodic maintenance.
5. **Archived agents can be removed.** If an agent is retired, remove its `nous/{id}/` directory. No cascade dependencies.

See `docs/ARCHITECTURE.md` for the oikos hierarchy and design rationale.
