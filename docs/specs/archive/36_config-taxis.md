# Spec 36: Config Taxis — 4-Layer Workspace Architecture + SecretRef Credential Storage

**Status:** Draft (partially superseded by Spec 44 — Oikos)
**Origin:** Issue #267
**Module:** `taxis`
**See also:** Spec 44 supersedes the 4-layer architecture with the oikos 3-tier hierarchy. SecretRef, exec tool config, deploy pipeline, and sidecar security concerns remain valid here.

---

## Problem

Deployment config is scattered across 4+ locations: `~/.aletheia/`, `~/.aletheia/credentials/`, workspace directories, and tool configs. No single place to manage a deployment. Credentials are stored as plaintext files with no resolution pattern.

## Design

### 4-Layer Architecture

Clean separation into four layers:

| Layer | Location | Contains |
|-------|----------|----------|
| Framework | `<operator-fork>/` | Runtime, shared tools, `_example/` template |
| Identity + workspace | `nous/` | Agent files, memory, scratch space |
| Team work | `<team-data-repo>/` | DDLs, dashboards, knowledge, standards |
| Deployment config | `deploy/` | Credentials, tool config, prosoche, bootstrap |

### Agent Scratch Space

```
nous/{agent}/workspace/
├── scripts/     # Investigation SQL, one-off queries (tracked)
├── data/        # Sample exports, debug output (.gitignored)
└── drafts/      # WIP before promotion to <team-data-repo> (tracked)
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

## Memory Sidecar Security (from #340)

The memory sidecar binds to `0.0.0.0` with auth that's never enforced — any LAN device can read/write/delete memories.

### Bind Address
Change from `0.0.0.0` to `127.0.0.1`. The sidecar serves only the local runtime.

### Token Generation at Init
`aletheia init` generates `ALETHEIA_MEMORY_TOKEN` and stores it alongside the session key in `deploy/credentials/` (or `~/.aletheia/`). The systemd service file passes it as an environment variable.

### Client Auth Wiring
`memory-client.ts` reads the token and sends `Authorization: Bearer <token>` on all requests. Affects: `recall.ts`, `finalize.ts`, `mem0-*.ts` tools.

### Doctor Validation
`aletheia doctor` checks: token exists, sidecar responds to authenticated request, bind address is localhost.

**Files:** `infrastructure/memory/sidecar/aletheia_memory/app.py`, `infrastructure/runtime/src/nous/recall.ts`, `infrastructure/runtime/src/nous/pipeline/stages/finalize.ts`, `infrastructure/runtime/src/organon/built-in/mem0-*.ts`

## Shell Injection in start.sh (from #342)

`start.sh` interpolates API key directly into a `python3 -c` string — shell metacharacters in the key value break the literal or enable injection. Fix: pass via environment variable instead of string interpolation.

**Files:** `shared/bin/start.sh`

## Systemd Unit Installation (from #343)

`deploy.sh` calls `systemctl restart aletheia` but no unit file exists. The script hard-fails and triggers rollback on every deployment. Two aspects:

### Unit File
Install a systemd user service file for the main runtime (prosoche already has a template). `aletheia init` should install it. Include: `WorkingDirectory`, `ExecStart`, `Restart=on-failure`, `StandardOutput=journal`.

### deploy.sh Update
Reference the installed unit. Health check via `curl -sf http://localhost:18789/health` after restart. Rollback on health check failure, not on systemctl exit code.

**Files:** `scripts/deploy.sh`, `scripts/rollback.sh`

---

## Phases

TBD — needs design review.
