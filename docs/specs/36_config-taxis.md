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

## Exec Tool Configuration (from #338)

Issue #338 identifies exec tool gaps that are workspace/config concerns (vs. context engineering concerns in Spec 35):

### Working Directory
- **Per-call `cwd` parameter:** Tools should accept an optional `cwd` that overrides the default. Currently all exec calls run from the nous workspace root.
- **Per-nous `workingDir` config:** `agents.list[id].workingDir` sets the default working directory for all tool calls. Resolved via 4-layer config. Useful when agents need to operate primarily from a different directory (e.g., the monorepo root).

### Default Timeout
Current default is 30s, which causes timeouts on builds, test runs, and long commands. Change to 120s default, configurable per-nous via `agents.list[id].pipeline.execTimeout`.

### Glob Tool
Add a dedicated `glob` tool (or extend `find`) that returns file lists matching glob patterns. Avoids the overhead of shelling out to `fd` or `find` for simple pattern matching.

## Deploy Pipeline (from #339)

Issue #339 identifies gaps in the deploy/build chain:

### npm install in Deploy
Current deploy script bundles with `tsdown` but doesn't run `npm install` — new dependencies require manual intervention. Deploy should: `git pull` → `npm install --production` → `npm run build` → restart.

### Anchor.json Scaffolding
`anchor.json` should be created during `bootstrap.sh` if missing. Currently requires manual creation.

### Agent Workspace Scaffolding
Deploy should verify `nous/{agent}/` directories exist for all configured agents, creating missing ones from `_example/` template.

### Systemd Config Fallback
If systemd user services aren't configured, deploy should offer to create them (or at least detect and warn).

---

## Phases

TBD — needs design review.
