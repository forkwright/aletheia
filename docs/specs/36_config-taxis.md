# Spec 36: Config Taxis — 4-Layer Workspace Architecture + SecretRef Credential Storage

**Status:** Draft
**Origin:** Issue #267
**Module:** `taxis`

---

## Problem

Deployment config is scattered across 4+ locations: `~/.aletheia/`, `~/.aletheia/credentials/`, workspace directories, and tool configs. No single place to manage a deployment. Credentials are stored as plaintext files with no resolution pattern.

## Design

### 4-Layer Architecture

Clean separation into four layers:

| Layer | Location | Contains |
|-------|----------|----------|
| Framework | `ergon/` | Runtime, shared tools, `_example/` template |
| Identity + workspace | `nous/` | Agent files, memory, scratch space |
| Team work | `ergon_tools/` | DDLs, dashboards, knowledge, standards |
| Deployment config | `deploy/` | Credentials, tool config, prosoche, bootstrap |

### Agent Scratch Space

```
nous/{agent}/workspace/
├── scripts/     # Investigation SQL, one-off queries (tracked)
├── data/        # Sample exports, debug output (.gitignored)
└── drafts/      # WIP before promotion to ergon_tools (tracked)
```

### Deploy Consolidation

```
deploy/
├── aletheia.json           # Main config (from ~/.aletheia/)
├── credentials/            # All secrets (from scattered locations)
├── prosoche.yaml           # Attention config
├── tools.yaml              # Tool overlay
├── contracts/              # Agent contracts
└── bootstrap.sh            # Replaces install.sh
```

`~/.aletheia/` becomes purely runtime state (sessions.db, logs, socket).

Config resolution via `ALETHEIA_CONFIG_DIR` env var in `start.sh`.

### SecretRef Credential Storage

Credentials referenced by name, resolved at runtime. No plaintext secrets in main config.

### Workspace Indexing

`workspace-index` updated to include `workspace/` so scratch scripts surface in pre-turn recall.

## Open Questions

- Migration path from current scattered layout
- Whether `deploy/` should be a separate repo or directory within monorepo
- SecretRef resolver implementation (env vars, Vault, file-based)

## Phases

TBD — needs design review.
