# Instance Scaffold

This directory defines the structure for an Aletheia deployment. To initialize:

```bash
aletheia init
# or manually:
cp -r instance.example instance
```

Then configure `instance/config/aletheia.toml` and add your agents under `instance/nous/`.
The starter config binds the gateway to localhost. For private LAN or tailnet
exposure, copy the named reference settings from
`config/aletheia.tailnet.toml` and keep authentication, CORS, and TLS explicit.

## Runtime environment file

The canonical environment file for a deployed instance is
`instance/config/env`. The root `.env.example` is the template for that file:

```bash
cp .env.example instance/config/env
chmod 600 instance/config/env
```

`ALETHEIA_ROOT` means the instance root only. It never points at the source tree
or an install prefix. Helper scripts that need an executable path use
`ALETHEIA_BIN` instead. The included systemd unit loads
`%h/aletheia/instance/config/env`, matching this layout.

## Directory Structure

```text
instance/
├── config/                     # Deployment configuration
│   ├── aletheia.toml           # Main config (copy from instance.example/config/aletheia.toml)
│   └── credentials/            # API keys, OAuth tokens, Signal creds
│
├── data/                       # Runtime data stores
│   ├── sessions.db/            # fjall-backed session store (directory, not a file)
│   ├── working-checkpoints.fjall/
│   │                           # fjall-backed <key_info> checkpoint store
│   ├── knowledge.fjall/        # fjall-backed knowledge store
│   ├── backups/                # Daemon-managed backup snapshots
│   └── archive/                # Archived sessions exported as JSON
│
├── logs/                       # Runtime log output
│
├── nous/                       # Agent identity + session memory ONLY
│   ├── _default/               # Pre-configured default agent (Pronoea/Noe)
│   │                           # Used when aletheia init creates the first agent
│   │                           # Copy to nous/{your-id}/ and update aletheia.toml
│   ├── _template/              # Blank template for new agents (copied by `aletheia add-nous`)
│   │   ├── SOUL.md             # Agent identity and character
│   │   ├── IDENTITY.md         # Name, emoji, avatar
│   │   ├── GOALS.md            # Goals and purpose
│   │   ├── MEMORY.md           # Memory configuration
│   │   └── memory/             # Session logs (memory/YYYY-MM-DD.md)
│   └── {agent-id}/             # Per-agent workspace
│       │                       # See docs/WORKSPACE_FILES.md for the full reference
│       ├── SOUL.md             # Identity (required)
│       ├── IDENTITY.md         # Name, emoji
│       ├── MEMORY.md           # Curated operational memory
│       └── memory/             # Session logs (YYYY-MM-DD.md)
│
├── shared/                     # Runtime infrastructure (agents only)
│   ├── bin/                    # Shared shell scripts and executables
│   ├── calibration/            # Model competence calibration data
│   ├── commands/               # Custom slash commands
│   ├── coordination/           # Cross-agent runtime state
│   │   ├── memory/             # Shared memory operations log
│   │   ├── prosoche/           # Attention/heartbeat state
│   │   ├── status/             # Agent status files
│   │   └── traces/             # Session trace logs (rotatable)
│   ├── docs/                   # Shared operational docs
│   ├── hooks/                  # Shared lifecycle hooks
│   ├── schemas/                # Shared data schemas
│   ├── skills/                 # Learned skills (auto-extracted, NOT in git)
│   ├── templates/              # Shared prompt/doc templates
│   └── tools/                  # Shared tool definitions
│
├── theke/                      # ALL working files (single shared tree)
│   ├── projects/               # Project-scoped work
│   │   ├── {project-name}/     # e.g. aletheia/, my-project/, mba/, homelab/
│   │   └── ...
│   ├── research/               # Research not tied to a specific project
│   ├── reference/              # Persistent reference material
│   ├── nous/                   # Agent-specific scratch space
│   │   └── {agent-id}/         # When an agent needs private working files
│   └── archive/                # Completed/historical (one place for all)
│
└── signal/                     # Signal-cli data directory
```

## Three-Tier Cascade

Resolution order (most specific wins):
1. `instance/nous/{id}/`: agent-specific overrides
2. `instance/shared/`: shared across all agents
3. `instance/theke/`: human + agent collaborative space

Tools, templates, hooks, and config all resolve through this cascade.

## What goes where

| Content | Location | Why |
|---------|----------|-----|
| Agent identity (SOUL.md, etc.) | `nous/{id}/` | Per-agent bootstrap files |
| Agent session logs | `nous/{id}/memory/` | Per-agent, daily files |
| Curated agent memory | `nous/{id}/MEMORY.md` | Operational context |
| Project work (plans, specs, etc.) | `theke/projects/{name}/` | Shared, subject-indexed |
| Research | `theke/research/` | Shared, discoverable |
| Reference material | `theke/reference/` | Persistent docs |
| Agent scratch files | `theke/nous/{id}/` | When genuinely agent-private |
| Completed/historical work | `theke/archive/` | One archive, not per-agent |
| Shared tools & scripts | `shared/tools/`, `shared/bin/` | Runtime infrastructure |
| Coordination state | `shared/coordination/` | Runtime traces, status |
| API keys, OAuth tokens | `config/credentials/` | Deployment secrets |
| Runtime databases | `data/` | fjall-backed session, working-checkpoint, and knowledge stores |

## Key Principles

1. **Organize by subject, not by agent.** Files go in `theke/projects/{name}/`, not `nous/{id}/docs/`. Any agent can find anything. One tree to search, one tree to prune.
2. **`nous/{id}/` is identity + memory only.** No docs/, drafts/, plans/, research/, or archive/ directories in agent workspaces. If it's not a bootstrap file or session log, it belongs in theke/.
3. **`theke/` is the single working filesystem.** Shared by default. The operator and all agents read/write here. Organized by what the work IS, not who's doing it.
4. **`shared/` is runtime infrastructure.** Scripts, coordination, and tools for the runtime, not browsable content.
5. **If it doesn't ship to a GitHub clone, it's instance-only.** Skills, credentials, agent workspaces, and traces are all instance-only.
6. **Coordination state is ephemeral.** Traces and status files can be rotated/purged without data loss.
7. **Archived agents can be removed.** If an agent is retired, remove its `nous/{id}/` directory. No cascade dependencies.

See `docs/ARCHITECTURE.md` for the oikos hierarchy and design rationale.
