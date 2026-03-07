# _example - Nous Agent Template

This directory is the reference template for new Aletheia agents. It shows the expected file and directory structure for a nous workspace.

## Directory Structure

```text
{agent-id}/
├── workspace/
│   ├── scripts/     # Agent-authored scripts and tools (git-tracked)
│   ├── drafts/      # Work-in-progress output files (git-tracked)
│   └── data/        # Datasets and ephemeral outputs (gitignored)
├── AGENTS.md        # Operational defaults and tool grants
├── CONTEXT.md       # Persistent context injected every session
├── GOALS.md         # Active and completed goals
├── IDENTITY.md      # Runtime-required: name and emoji
├── MEMORY.md        # Agent knowledge base
├── PROSOCHE.md      # Attention and focus directives
├── SOUL.md          # Identity, voice, principles (written during onboarding)
├── TOOLS.md         # Tool usage guidelines
└── USER.md          # Operator profile and preferences
```

## Gitignore Defaults

The parent `nous.dir/.gitignore` (written by `aletheia init`) contains:

- `*/workspace/plans/` - planning artifacts (large, ephemeral)
- `*/workspace/data/` - datasets and outputs (potentially large, private)
- `memory/` - session memory logs
- `.aletheia-index/` - workspace file index
- `.env`, `*.key`, `*.pem`, `*.secret`, `secrets/`, `credentials/` - credentials

`workspace/scripts/` and `workspace/drafts/` are git-tracked by default.

## Usage

`aletheia init` creates a new agent directory with this structure. Files from `_example/` are NOT copied - the new agent starts with blank files generated from templates in the runtime. The `workspace/` subdirectories are created automatically.
