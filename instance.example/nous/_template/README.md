# Nous agent template

This directory is the reference template for new Aletheia agents. Copy it with `aletheia add-nous <name>` to create an additional agent.

## Directory structure

```text
{agent-id}/
├── SOUL.md          # Identity, voice, principles
├── IDENTITY.md      # Runtime-required: name and emoji
├── GOALS.md         # Active and completed goals
├── MEMORY.md        # Curated operational memory
├── AGENTS.md        # Operational rules, delegation, output quality
├── CONTEXT.md       # Session-scoped state (runtime-written)
├── PROSOCHE.md      # Attention and heartbeat directives
├── TOOLS.md         # Tool inventory (auto-generated)
├── USER.md          # Operator profile (learned from conversation)
├── VOICE.md         # Operator writing style (learned from conversation)
└── USER.md          # Operator profile and preferences
```

## What goes where

This directory holds **identity and session memory only**. All working files go in `theke/`:

| Content | Location |
|---------|----------|
| Identity files (SOUL, GOALS, etc.) | `nous/{id}/` |
| Session logs | `nous/{id}/memory/YYYY-MM-DD.md` |
| Project work, drafts, specs | `theke/projects/{name}/` |
| Research | `theke/research/` |
| Agent scratch space | `theke/nous/{id}/` |

See `instance.example/README.md` for the full file organization guide.

## Gitignore defaults

- `memory/` session logs
- `.aletheia-index/` workspace file index
- `.env`, `*.key`, `*.pem`, `*.secret`, `secrets/`, `credentials/`

## Usage

`aletheia add-nous <name>` copies this template to create a new agent. `aletheia init` uses `_default/` (Pronoea) for the first agent.
