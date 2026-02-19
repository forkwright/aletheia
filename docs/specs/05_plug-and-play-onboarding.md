# Spec: Plug-and-Play Onboarding

**Status:** Draft  
**Author:** Chiron  
**Date:** 2026-02-19  

---

## Problem

Aletheia requires too much tribal knowledge to deploy. The current setup involves:

1. **7 manual steps** in the quickstart, several requiring the operator to understand internal architecture (what are bindings? where do workspace paths go? what's a session scope?)
2. **3 separate config surfaces** — `.env`, `aletheia.json`, and per-agent workspace files — with no validation that they're consistent
3. **No guided onboarding** — creating an agent means copying `_example/`, editing 5+ markdown files, adding entries to `aletheia.json`, and adding Signal bindings, all by hand
4. **No single startup/shutdown** — the operator must independently manage docker containers (Qdrant, Neo4j, signal-cli), the gateway process, and optionally the memory sidecar and prosoche daemon
5. **No config validation** — a typo in a workspace path, a missing env var, or a malformed binding silently fails at runtime

The result: only the person who built the system can operate it. This blocks team adoption and makes forking for organizational use (e.g., Ergon) high-overhead.

### Goal

One setup command for first run. One start command. One stop command. Agents managed via config and web UI. A new team member can go from clone to running in under 5 minutes with no knowledge of internals.

---

## Design Principles

### Config is the source of truth

Everything that defines "this instance" lives in config files. No runtime state that can't be reconstructed from config + data. An operator should be able to read the config and understand the full system topology.

### Progressive disclosure

First run asks only what's required: API key, instance name, first agent. Everything else has sensible defaults. Advanced config (prosoche, routing tiers, cron jobs, custom tools) is available but never required.

### Validate early, fail loud

Every config surface is validated at startup. Missing env vars, unreachable services, malformed agent configs, broken workspace paths — all caught before the system accepts its first message. Error messages say what's wrong AND how to fix it.

### One process tree

`aletheia up` starts everything. `aletheia down` stops everything. The operator doesn't need to know about docker, systemd, or process management unless they want to.

---

## Architecture

### CLI as the Control Surface

```
aletheia setup              # Interactive first-run wizard
aletheia up                 # Start all services
aletheia down               # Stop all services  
aletheia status             # Health of all components
aletheia agent add          # Create a new agent (interactive or flags)
aletheia agent list         # Show all agents and their status
aletheia agent remove <id>  # Remove an agent
aletheia config validate    # Check config without starting
aletheia config show        # Dump resolved config (with defaults applied)
aletheia logs [service]     # Tail logs (gateway, memory, signal, agent:<id>)
```

### `aletheia setup` — First-Run Wizard

Interactive, idempotent (safe to re-run). Handles all first-time configuration:

```
$ aletheia setup

Welcome to Aletheia.

1. API Key
   Enter your Anthropic API key: sk-ant-...
   ✓ Key validated (claude-sonnet-4-6 accessible)

2. Instance
   Instance name [aletheia]: ergon
   Data directory [~/.aletheia]: 
   ✓ Created ~/.aletheia/{config,data,logs}

3. Signal (optional)
   Configure Signal messaging? [y/N]: y
   Phone number: +1...
   → Starting signal-cli container for registration...
   → Scan QR code or enter verification code: ...
   ✓ Signal account linked

4. First Agent
   Agent name: Chiron
   Agent emoji [🤖]: 🏹
   Agent role (one line): Data engineering, dashboards, schema governance
   → Created nous/chiron/ with workspace files
   → Added to config with default model (claude-sonnet-4-6)
   
   Bind to Signal DM? [Y/n]: y
   ✓ Binding created: Signal DM → chiron

5. Memory
   → Starting Qdrant and Neo4j containers...
   ✓ Qdrant healthy on :6333
   ✓ Neo4j healthy on :7687

6. Web UI
   → Building UI...
   ✓ Available at http://localhost:18789/ui
   Gateway token: <generated>

Setup complete. Run 'aletheia up' to start.
```

Each step is independently skippable and re-runnable. State is persisted in `~/.aletheia/setup.state` so re-running skips completed steps (with `--force` to redo).

### `aletheia up` / `aletheia down`

Manages the full process tree:

```
aletheia up
├── docker compose up -d          # Qdrant, Neo4j, signal-cli
├── wait for healthy (with timeout)
├── validate config
├── start gateway                 # Node.js process
├── start memory sidecar          # If configured  
├── start prosoche                # If configured
└── print status summary
```

```
aletheia down
├── stop prosoche
├── stop memory sidecar
├── stop gateway (graceful: finish in-flight turns, then SIGTERM)
└── docker compose down
```

Implementation: a shell script or Node.js orchestrator that wraps docker compose and process management. For systemd environments, `aletheia install-service` generates a unit file that calls `aletheia up`.

### Process Management

Two options (pick one during implementation):

**Option A: Shell orchestrator.** `aletheia up` is a bash script that starts processes, writes PIDs to `~/.aletheia/pids/`, and `aletheia down` reads them. Simple, portable, easy to debug.

**Option B: Process manager.** Use the gateway itself as the process supervisor. The gateway spawns and monitors child processes (sidecar, prosoche), restarts on crash, and `aletheia down` just stops the gateway which cascades. More robust, more complex.

Recommendation: **Option A for v1.** Shell script. Keep it simple. The gateway shouldn't be a process manager — that's scope creep.

---

## Config Consolidation

### Current State (3 surfaces)

| Surface | Location | Contains |
|---------|----------|----------|
| `.env` | `shared/config/aletheia.env` | API keys, service URLs, paths |
| `aletheia.json` | `~/.aletheia/aletheia.json` | Agents, bindings, channels, gateway, plugins |
| Workspace files | `nous/<id>/*.md` | Per-agent character, tools, memory, goals |

### Proposed State (2 surfaces, clear boundary)

| Surface | Location | Contains | Edited by |
|---------|----------|----------|-----------|
| `aletheia.yaml` | `~/.aletheia/aletheia.yaml` | Everything: secrets, agents, bindings, channels, services | CLI, web UI, hand-edit |
| Workspace files | `nous/<id>/*.md` | Agent character and working memory | Agent, operator |

**Key changes:**

1. **Merge `.env` into config.** API keys and service URLs move into `aletheia.yaml` under a `secrets` and `services` section. The separate `.env` file is eliminated. Environment variable overrides still work (for CI/containers) but aren't the primary config surface.

2. **YAML over JSON.** Supports comments, multi-line strings, and is more readable for operators who aren't developers. JSON remains accepted (auto-detected by extension).

3. **Agent definitions include workspace path.** No separate step to "add agent to config" — the agent entry in `aletheia.yaml` IS the registration. The workspace directory is created from it.

4. **Schema validation.** A JSON Schema for `aletheia.yaml` enables IDE autocomplete and catches errors before runtime. `aletheia config validate` runs this.

### Config Structure

```yaml
# ~/.aletheia/aletheia.yaml

instance:
  name: ergon
  dataDir: ~/.aletheia/data      # Qdrant, Neo4j volumes, logs
  timezone: America/New_York

secrets:
  anthropic: sk-ant-...
  voyage: vo-...                   # Optional: memory embeddings
  brave: ...                       # Optional: web search
  perplexity: ...                  # Optional: research

services:
  qdrant:
    host: localhost
    port: 6333
    managed: true                  # aletheia up/down controls this
  neo4j:
    host: localhost
    port: 7687
    auth: neo4j/changeme
    managed: true
  signal:
    enabled: true
    account: "+15125551234"
    managed: true                  # Docker container lifecycle

gateway:
  port: 18789
  bind: loopback
  auth:
    mode: token
    token: <auto-generated>
  ui: true

agents:
  defaults:
    model: claude-sonnet-4-6
    fallbacks: [claude-haiku-4-5-20251001]
    contextTokens: 200000
    tools: full

  list:
    - id: chiron
      name: Chiron
      emoji: 🏹
      workspace: ./nous/chiron     # Relative to repo root
      model: claude-opus-4-6       # Override default
      bindings:
        - channel: signal
          peer: dm                 # Default DM binding
        - channel: web

    - id: hermes
      name: Hermes
      emoji: 🪽
      workspace: ./nous/roi
      bindings:
        - channel: web             # Web only, no Signal

# Optional subsystems
prosoche:
  enabled: false
  interval: 45m
  schedule: "08:00-18:00 America/New_York weekdays"

cron:
  enabled: false
  jobs: []
```

**Bindings simplified.** Instead of a separate top-level `bindings` array with match objects, bindings are inline on the agent definition. `peer: dm` means "bind this agent to the operator's Signal DM." Named group bindings use `peer: group:<name>`. The config layer resolves Signal UUIDs from the linked account — the operator never touches UUIDs.

---

## Agent Management

### Via CLI

```bash
# Interactive
$ aletheia agent add
Agent ID (lowercase, no spaces): atlas
Agent name: Atlas  
Emoji [🤖]: 🗺️
Role: Schema governance and data documentation
Model [claude-sonnet-4-6]: 
Bind to Signal DM? [y/N]: n
Bind to web UI? [Y/n]: y
→ Created nous/atlas/ with workspace template
→ Added to aletheia.yaml
→ Hot-reloaded gateway (no restart needed)
✓ Atlas is live at http://localhost:18789/ui

# Non-interactive  
$ aletheia agent add --id atlas --name Atlas --emoji 🗺️ --model claude-sonnet-4-6 --bind web

# Remove
$ aletheia agent remove atlas
Remove agent 'Atlas'? This deletes the workspace. [y/N]: y
→ Removed from aletheia.yaml
→ Archived workspace to ~/.aletheia/archive/atlas-2026-02-19/
→ Hot-reloaded gateway
✓ Atlas removed
```

### Via Web UI

The control UI (existing at `/ui`) gains an **Agent Management** panel:

- List all agents with status (online, idle, in-turn, error)
- Create new agent (same fields as CLI wizard)
- Edit agent config (model, bindings, tools profile)
- Remove agent (with archive)
- View agent health: last turn time, token usage, memory stats, error rate
- **Cannot edit workspace markdown files** — those belong to the agent and operator, not the UI

### Hot Reload

Agent changes (add, remove, config edit) take effect without restarting the gateway. The gateway watches `aletheia.yaml` for changes and reloads agent definitions. Workspace file changes are picked up on next turn (already the case).

Implementation: `fs.watch` on `aletheia.yaml` + debounce. On change: re-validate, diff agent list, register new agents, deregister removed ones, update config on existing ones.

---

## Startup Validation

`aletheia up` (and `aletheia config validate`) runs a preflight check:

```
$ aletheia up

Preflight checks:
  ✓ Config loaded from ~/.aletheia/aletheia.yaml
  ✓ Anthropic API key valid
  ✓ Node.js >= 22.12
  ✓ Docker available
  
Starting services:
  ✓ Qdrant healthy (localhost:6333)
  ✓ Neo4j healthy (localhost:7687)  
  ✓ signal-cli healthy (localhost:8080)

Validating agents:
  ✓ chiron — workspace exists, SOUL.md present, IDENTITY.md valid
  ✓ hermes — workspace exists, SOUL.md present, IDENTITY.md valid
  ⚠ hermes — no MEMORY.md (will create empty)

Starting gateway:
  ✓ Gateway listening on :18789
  ✓ Web UI available at http://localhost:18789/ui
  ✓ 2 agents registered, 3 bindings active

Aletheia is running. Logs: aletheia logs
```

### Failure Modes

| Check | Failure | Action |
|-------|---------|--------|
| API key missing | Fatal | Print setup instructions, exit |
| API key invalid | Fatal | Print "check key at console.anthropic.com", exit |
| Docker not running | Fatal if managed services | Print "start Docker or set managed: false", exit |
| Qdrant unreachable | Fatal | Print connection details, suggest `docker compose up` |
| Neo4j unreachable | Warning | Memory extraction degraded, continue |
| Signal not linked | Warning | Signal disabled, web-only mode |
| Agent workspace missing | Warning | Create from template, continue |
| Agent SOUL.md missing | Warning | Create minimal default, continue |
| Port in use | Fatal | Print what's using the port, exit |

Principle: **degrade gracefully where possible, fail hard where continuing would corrupt state.**

---

## Workspace Template

`aletheia agent add` creates a workspace from a template. The template is `nous/_example/` with the following modifications:

**Generated files** (populated from CLI/UI input):
- `IDENTITY.md` — name and emoji
- `SOUL.md` — scaffolded with name, role description, and prompts for the operator to customize

**Empty-but-present files** (operator fills in over time):
- `AGENTS.md` — operational instructions (pre-filled with standard sections)
- `MEMORY.md` — starts empty, agent populates
- `GOALS.md` — starts with a single "Get started" goal
- `TOOLS.md` — pre-filled with available shared tools
- `USER.md` — copied from instance-level `USER.md` if it exists, otherwise scaffolded

**Created directories:**
- `memory/` — for session logs
- `references/` — for domain documents
- `outputs/` — for deliverables

The template is customizable: operators can modify `nous/_example/` to change what new agents get.

---

## Migration Path

### From Current Aletheia (aletheia.json + .env)

```bash
$ aletheia setup --migrate

Detected existing configuration:
  ~/.aletheia/aletheia.json (5 agents, 8 bindings)
  shared/config/aletheia.env (12 variables)

Migrating to aletheia.yaml...
  ✓ Merged env vars into secrets/services sections
  ✓ Converted agent list (JSON → YAML)
  ✓ Inlined bindings on agent definitions
  ✓ Preserved all custom overrides

  Backed up originals:
    ~/.aletheia/aletheia.json → ~/.aletheia/archive/aletheia.json.pre-migration
    shared/config/aletheia.env → ~/.aletheia/archive/aletheia.env.pre-migration

  Review: ~/.aletheia/aletheia.yaml
  Validate: aletheia config validate
```

### From Ergon Fork

Same migration, plus:
- Detect Summus-specific config (Redshift connection, Hex API)
- Move to an `extensions` or `integrations` section in the YAML
- Preserve nous workspace files as-is

---

## Existing Infrastructure Audit

Before building, here's what already exists and what state it's in. The spec must replace or extend — not duplicate.

### Config System — `taxis/`
**Status: Solid foundation, needs extension not replacement.**

| Component | Exists | State | Spec Impact |
|-----------|--------|-------|-------------|
| Config loading | ✅ `taxis/loader.ts` | Reads JSON from `~/.aletheia/aletheia.json`, validates with Zod | Add YAML support + env merge. Don't rewrite — extend `loadConfig()` to detect `.yaml` vs `.json` |
| Zod schema | ✅ `taxis/schema.ts` | ~350 lines, comprehensive. Agents, bindings, channels, gateway, plugins, session, cron, watchdog, branding, MCP, env vars, models, providers | **Already has** most of what the spec's `aletheia.yaml` proposes. Add `secrets`, `profiles`, `backup`, `limits` sections. Don't flatten bindings into agents — the current separate `bindings[]` array is already working and more flexible |
| Path resolution | ✅ `taxis/paths.ts` | Hardcoded `ALETHEIA_ROOT` fallback to `/mnt/ssd/aletheia`. Uses `~/.aletheia/` for config dir | Fine as-is |
| Config validation | ✅ `aletheia doctor` in `entry.ts` | Loads config, prints summary. No deep validation (services reachable, workspaces exist) | Extend `doctor` → add preflight checks from spec |
| Env var injection | ✅ `loader.ts:applyEnv()` | Config can set env vars via `env.vars` section. Existing env takes precedence | This partially replaces the `.env` file already. Document it. |
| Unknown key warnings | ✅ `loader.ts:warnUnknownKeys()` | Warns on unknown top-level and per-nous keys | Good defensive behavior, keep |

**Key insight:** The config schema already supports `env.vars` for injecting API keys and paths. The `.env` file isn't strictly necessary today — it's just convention. The spec should formalize this rather than invent a new `secrets` section. Use `env.vars` for secrets with env var override support (`${ANTHROPIC_API_KEY}`).

**Binding simplification: DON'T DO IT.** The current `bindings[]` array is more powerful than inline-on-agent. It supports multiple accounts, group routing, and account-specific policies. The spec's inline proposal would lose flexibility. Instead: add sugar for common cases in `aletheia setup` but keep the underlying model.

### CLI — `entry.ts`
**Status: Already has more than the spec assumes.**

| Command | Exists | Notes |
|---------|--------|-------|
| `aletheia gateway start` | ✅ | Main entry point. Also `gateway run` (systemd alias) |
| `aletheia doctor` | ✅ | Config validation. Rename to `config validate` or keep as `doctor` |
| `aletheia status` | ✅ | Hits `/api/metrics`. Shows uptime, per-agent tokens/sessions/messages, cache hit rate, services health, cron jobs |
| `aletheia send` | ✅ | Send message to agent from CLI. Shows response + token usage |
| `aletheia sessions` | ✅ | List sessions with status/message counts |
| `aletheia cron list` | ✅ | List cron jobs with schedule/next run |
| `aletheia cron trigger` | ✅ | Manually trigger a cron job |
| `aletheia replay` | ✅ | Replay session history. `--live` re-sends messages and compares |
| `aletheia setup` | ❌ | Not yet |
| `aletheia up/down` | ❌ | Not yet — gateway starts itself but doesn't manage Docker |
| `aletheia agent add/remove` | ❌ | Not yet |
| `aletheia backup/restore` | ❌ | Not yet |
| `aletheia upgrade` | ❌ | Not yet |

**Key insight:** The CLI is already Commander-based with proper option parsing. New commands slot in cleanly. `aletheia status` already does most of what the spec describes — it just doesn't manage the process lifecycle.

### Observability
**Status: Surprisingly complete. The spec underestimates what's already built.**

| Feature | Exists | Where |
|---------|--------|-------|
| Per-agent token tracking | ✅ | `mneme/store.ts` — `usageByNous` with input/output/cache_read/cache_write/turns per agent |
| Cost calculation | ✅ | `hermeneus/pricing.ts` — per-model pricing with breakdown (input/output/cache) |
| Cost API endpoints | ✅ | `/api/costs/summary`, `/api/costs/agent/:id`, `/api/costs/session/:id` — full cost attribution |
| Turn tracing | ✅ | `nous/trace.ts` — `TurnTrace` with tool calls, cross-agent calls, bootstrap files, token usage, latency. Persisted as JSONL |
| Web UI metrics dashboard | ✅ | `MetricsView.svelte` — uptime, tokens, cache hit rate, turns, total cost, per-agent table (sessions, messages, tokens, cost), services health badges, cron status |
| Service health monitoring | ✅ | `daemon/watchdog.ts` — probes services, tracks state transitions, sends alerts on down/recovery. 12hr re-alert interval |
| Real-time event streaming | ✅ | `eventBus` emits `turn:before`, `turn:after`, `tool:called`, `tool:failed` → SSE to UI |
| Agent activity | ✅ | `lastActivity` tracked per agent in metrics |
| Per-session cost drill-down | ✅ | `/api/costs/session/:id` — per-turn cost breakdown with model info |

**Key insight:** The spec's "Observability Dashboard" gap (§1) is largely already built. What's missing is:
- Historical cost data (current metrics are in-memory, reset on restart)
- Burn rate projections
- Daily/weekly/monthly aggregation
- Exportable cost reports
- Alert thresholds on cost (the `limits` section in the spec)

### Plugin System — `prostheke/`
**Status: Functional. Addresses most of the spec's "Extension System" need.**

| Feature | Exists | Notes |
|---------|--------|-------|
| Plugin manifest | ✅ | `aletheia.plugin.json` with id, name, version, description |
| Plugin hooks | ✅ | `onStart`, `onShutdown`, `onBeforeTurn`, `onAfterTurn`, `onBeforeDistill`, `onAfterDistill`, `onConfigReload` |
| Plugin tools | ✅ | Plugins can register tool handlers |
| Plugin loading | ✅ | `prostheke/loader.ts` loads from paths in `plugins.load.paths` config |
| Plugin config | ✅ | `plugins.entries[id].config` — per-plugin config object |
| MCP client | ✅ | `organon/mcp-client.ts` — connects to external MCP servers (stdio/http/sse), registers their tools |

**Key insight:** The spec's "Extension System" (§5) is already the plugin system + MCP client. For Ergon's Redshift/Hex tools: either (a) write an Aletheia plugin with tools, or (b) expose them as MCP servers. The MCP path is more portable and already works. The spec should reference existing plugin/MCP infrastructure rather than proposing a new `extensions/` mechanism.

### What's Actually Missing (Revised Gap List)

After auditing, the real gaps are narrower than the spec originally claimed:

| # | Gap | Actually Missing? |
|---|-----|-------------------|
| 1 | Observability dashboard | **Partially built.** Missing: persistent cost history, burn rate, daily aggregation, export. UI exists. |
| 2 | Backup and restore | **Yes, fully missing.** No mechanism for state snapshots. |
| 3 | Agent self-test | **Yes, fully missing.** `doctor` validates config but never sends a test prompt. |
| 4 | Environment profiles | **Yes, missing.** No profile/overlay system. |
| 5 | Extension system | **Already exists** as plugins + MCP. Needs documentation, not new code. |
| 6 | Upgrade path | **Yes, fully missing.** No automated upgrade. |
| 7 | First-turn onboarding | **Yes, missing.** No bootstrap prompt for new agents. |
| 8 | Rate limiting / cost guards | **Partially exists.** `gateway.rateLimit.requestsPerMinute` exists but is HTTP-level, not per-agent token/cost budgets. |
| — | `aletheia up/down` | **Yes, fully missing.** Process lifecycle management. |
| — | `aletheia setup` | **Yes, fully missing.** Interactive wizard. |
| — | `aletheia agent add/remove` | **Yes, fully missing.** Agent CRUD. |
| — | Hot config reload | **Yes, missing.** No `fs.watch` or reload mechanism. |
| — | YAML config support | **Missing.** JSON only. |

---

## Gaps Identified After Research

After reviewing Dify, n8n, LangChain/LangGraph, OpenWebUI, Supabase CLI patterns, and Microsoft's agent onboarding research, the following capabilities are missing from the initial spec. Each represents industry best practice or an emerging standard that Aletheia should meet or exceed.

### 1. Observability Dashboard (Built-In)

Dify ships built-in monitoring with Langfuse/LangSmith integration. Aletheia already tracks token usage internally but doesn't surface it. The web UI should include:

- **Per-agent cost tracking.** Token usage (input/output/cache) per agent, per day, with running totals and burn rate projections. This is table stakes for any team deployment — leadership will ask "what does this cost?"
- **Turn-level tracing.** Each turn logged with: timestamp, agent, model used, token count, tool calls made, latency, outcome (success/error/timeout). Browsable in UI, exportable.
- **System health dashboard.** Single pane: Qdrant, Neo4j, signal-cli, gateway, sidecar, prosoche — each with status, uptime, last error. Already partially exists via `/api/status` and watchdog, but not visualized.
- **Agent activity timeline.** When each agent last responded, average turn latency, error rate over time. Answers "is this agent working?" without checking logs.

```
aletheia status --detail     # CLI version of the dashboard
```

Add to Phase 1 (`aletheia status`) as CLI-only, Phase 4 as web UI visualization.

### 2. Backup and Restore

No platform in the survey handles this well — which is exactly why Aletheia should. Agent state is distributed across workspace files, Qdrant vectors, Neo4j graph, and session history. Losing any piece breaks continuity.

```
aletheia backup [--output path]     # Snapshot everything
aletheia restore <backup-file>      # Restore from snapshot
```

**What gets backed up:**
- `aletheia.yaml` (config)
- All `nous/*/` workspace directories
- Qdrant collections (vector snapshots)
- Neo4j database dump
- Session history (SQLite or whatever the store is)

**Format:** Single tarball with manifest. Versioned so restore can detect incompatible snapshots.

**Automated:** `aletheia.yaml` gains a `backup` section:
```yaml
backup:
  enabled: true
  schedule: "0 3 * * *"    # Daily at 3 AM
  retain: 7                 # Keep 7 days
  path: ~/.aletheia/backups
```

This is the feature nobody builds until they lose data. Build it first.

### 3. Agent Health Checks and Self-Test

Beyond service-level health (Qdrant up? Neo4j up?), agents themselves need validation:

```
aletheia agent test <id>       # Send a test prompt, verify response
aletheia agent test --all      # Test every agent
```

**What it validates:**
- Workspace files parseable (IDENTITY.md has name/emoji, SOUL.md non-empty)
- Model accessible (API key valid, model name resolves)
- Tools functional (can the agent execute `exec`, `read`, etc.?)
- Memory reachable (can the agent search Qdrant?)
- Round-trip latency within bounds

**Smoke test on startup:** Optional (off by default, `--smoke-test` flag on `aletheia up`). Sends a minimal prompt to each agent and verifies a coherent response. Catches model access issues, broken tool configs, and malformed workspace files before the operator discovers them in conversation.

### 4. Environment Profiles

Dify and Supabase both support environment-aware configuration. Aletheia should too:

```yaml
# aletheia.yaml
profiles:
  development:
    agents.defaults.model: claude-haiku-4-5-20251001    # Cheap for dev
    gateway.bind: loopback
  production:
    agents.defaults.model: claude-sonnet-4-6
    gateway.bind: 0.0.0.0
    backup.enabled: true
```

```
aletheia up --profile production
ALETHEIA_PROFILE=production aletheia up
```

This avoids the common mistake of running expensive models during development or exposing the gateway externally on a dev box.

### 5. Plugin / Extension System for Org-Specific Tools

**UPDATE: This already exists.** The `prostheke/` plugin system supports plugin manifests, lifecycle hooks, tool registration, and per-plugin config. The MCP client (`organon/mcp-client.ts`) can connect to external tool servers.

For org-specific tools (Redshift, Hex), the path is:
- **Option A (MCP):** Write an MCP server for each tool, reference in `mcp.servers` config. Most portable.
- **Option B (Plugin):** Write an Aletheia plugin with `aletheia.plugin.json` manifest. Tighter integration, access to hooks.
- **Option C (shared/bin):** Keep tools as shell scripts in `shared/bin/` on PATH. Simplest, already how Ergon works.

What's actually needed is **documentation and a quickstart guide** for writing plugins/MCP servers, not new infrastructure. The spec should include a "Writing Your First Plugin" section rather than proposing a new `extensions/` directory.

### 6. Upgrade Path

Dify's upgrade story is "check if .env.example changed and update yours." That's the minimum. Aletheia should be better:

```
aletheia upgrade                    # Pull latest, rebuild, migrate config
aletheia upgrade --check            # Preview what would change
```

**What it does:**
1. Git pull (or download release)
2. Detect config schema changes between versions
3. Auto-migrate `aletheia.yaml` (add new defaults, deprecate removed keys)
4. Rebuild runtime (`npx tsdown`)
5. Rebuild UI (`npm run build`)
6. Run database migrations if any
7. Restart services

**What it doesn't do:** Overwrite workspace files, lose agent memory, or change secrets.

### 7. First-Turn Onboarding for New Agents

Microsoft's research emphasizes that agent onboarding isn't just config — it's ensuring the agent is **effective** once deployed. After `aletheia agent add`, the agent's first conversation should include a self-orientation:

- Agent reads its own workspace files and confirms understanding
- Agent runs `aletheia agent test` equivalent internally
- Agent writes an initial `MEMORY.md` entry: "I am [name], created on [date], my role is [role]"
- Agent proactively asks the operator what it should know

This is the difference between "agent exists" and "agent is ready." It can be a simple bootstrap prompt injected on first turn:

```
You have just been created. Read your workspace files, confirm you understand 
your role, test your tools, and introduce yourself to the operator.
```

### 8. Rate Limiting and Cost Guards

For team deployments, operators need guardrails:

```yaml
agents:
  defaults:
    limits:
      maxTurnsPerHour: 60
      maxTokensPerDay: 500000        # ~$7.50/day on Sonnet
      maxToolCallsPerTurn: 25
      alertAt: 80%                   # Warn at 80% of daily budget
```

The gateway enforces these. When a limit is hit: graceful degradation (queue, not crash). Alert via Signal/webhook. This prevents a runaway agent from burning through API credits — which WILL happen on a team deployment.

---

## Open Questions

1. **Secrets management.** `aletheia.yaml` with plaintext API keys is fine for single-user, but awkward for team repos. Options: (a) `secrets` section supports env var references (`anthropic: ${ANTHROPIC_API_KEY}`), (b) separate `secrets.yaml` in `.gitignore`, (c) OS keychain integration. Leaning toward (a) with (b) as the default for team setups.

2. **Docker dependency.** `managed: true` assumes Docker. For operators who run Qdrant/Neo4j natively or on remote hosts, `managed: false` skips container lifecycle. But should `aletheia setup` handle both paths, or assume Docker?

3. **Multi-user config.** For team forks: does each team member get their own `aletheia.yaml`, or is there a shared config with per-user overrides? The agent list is shared; the secrets are per-user. This might need a `local.yaml` overlay pattern.

4. **Web UI scope.** The spec proposes agent CRUD in the web UI. Should the UI also handle config editing (model selection, binding changes)? Or is that CLI/file-only to avoid the UI becoming a config management system?

5. **Signal account sharing.** For team deployments, does every agent share one Signal number, or can different agents have different numbers? Current architecture supports multiple accounts — the config should expose this cleanly.

6. **Workspace file ownership.** `SOUL.md` and `USER.md` are operator-authored. `MEMORY.md` and session logs are agent-authored. Should `aletheia agent add` distinguish these (scaffold operator files, leave agent files empty)?

---

## Modularity Pass — Hardcoded Values to Extract

The following are scattered throughout the codebase as magic constants. Each should become config-driven or at minimum centralized into a single constants file. Grouped by risk and effort.

### Service URLs (High Priority — Blocks Portability)

The memory sidecar URL is defined **4 different ways** across the codebase:

| File | Variable | Value |
|------|----------|-------|
| `nous/recall.ts` | `ALETHEIA_MEMORY_URL` | `http://127.0.0.1:8230` |
| `organon/built-in/fact-retract.ts` | `ALETHEIA_MEMORY_URL` | `http://127.0.0.1:8230` |
| `organon/built-in/mem0-search.ts` | `ALETHEIA_MEMORY_URL` | `http://127.0.0.1:8230` |
| `pylon/mcp.ts` | *(hardcoded inline)* | `http://127.0.0.1:8230` |
| `pylon/server.ts` | `MEMORY_SIDECAR_URL` | `http://127.0.0.1:8230` |

**Fix:** Single `services.memory.url` in config, resolved once at startup, injected into all consumers. The env var names are inconsistent (`ALETHEIA_MEMORY_URL` vs `MEMORY_SIDECAR_URL`) — unify.

Similarly, `aletheia.ts` hardcodes watchdog URLs for infrastructure services:
```typescript
{ name: "qdrant", url: "http://127.0.0.1:6333/healthz" },
{ name: "neo4j", url: "http://127.0.0.1:7474" },
{ name: "mem0-sidecar", url: "http://127.0.0.1:8230/health" },
{ name: "ollama", url: "http://127.0.0.1:11434/api/tags" },
```

These should come from the `watchdog.services` config that already exists but is being bypassed with hardcoded fallbacks.

### Default Path: `/mnt/ssd/aletheia` (High Priority — Blocks Forks)

This path appears in **30+ files** across runtime, scripts, services, and docs. It's the #1 blocker for anyone deploying on a machine that isn't your NUC.

| Location | Count | Fix |
|----------|-------|-----|
| `taxis/paths.ts` | 1 | Change fallback to `process.cwd()` or require `ALETHEIA_ROOT` |
| Python scripts (`shared/bin/`, `infrastructure/memory/scripts/`) | ~15 | All use `os.environ.get("ALETHEIA_ROOT", "/mnt/ssd/aletheia")` — acceptable but should fail explicitly if unset rather than silently falling back to a path that doesn't exist |
| `docker-compose.yml` volumes | 2 | Use `${ALETHEIA_DATA_DIR:-./data}` variable |
| Prosoche service file | 3 | Generate from template during `aletheia install-service` |
| Docs | ~8 | Replace with `/path/to/aletheia` placeholder |

**Fix:** `aletheia setup` writes `ALETHEIA_ROOT` to a well-known location (`~/.aletheia/env`). All scripts source it. Docker compose uses env var substitution. Prosoche service file generated at install time. The fallback in `paths.ts` changes from `/mnt/ssd/aletheia` to `process.cwd()` with a warning.

### Behavioral Thresholds (Medium Priority — Tuning Knobs)

These are scattered across modules as file-level constants. They're reasonable defaults but should be tunable without code changes:

| Constant | File | Value | Should be |
|----------|------|-------|-----------|
| `MAX_CONSECUTIVE_FAILURES` | `watchdog.ts` | 100 | Config: `watchdog.maxConsecutiveFailures` |
| `RE_ALERT_INTERVAL_MS` | `watchdog.ts` | 43200000 (12h) | Config: `watchdog.reAlertIntervalMs` |
| `MAX_TOOL_RESULT_CHARS` | `chunked-summarize.ts` | 8000 | Config: `agents.defaults.compaction.maxToolResultChars` |
| `MAX_CONCURRENT` (ephemeral) | `ephemeral.ts` | 3 | Config: `agents.defaults.ephemeral.maxConcurrent` |
| `MAX_PAGES` (browser) | `browser.ts` | 3 | Config: `tools.browser.maxPages` |
| `PAGE_TIMEOUT` (browser) | `browser.ts` | 30000 | Config: `tools.browser.timeoutMs` |
| `MAX_MESSAGE_LENGTH` | `message.ts` | 4000 | Config: `channels.signal.textChunkLimit` (already exists!) |
| `MAX_PENDING_SENDS` | `sessions-send.ts` | 5 | Config: `session.agentToAgent.maxPendingSends` |
| `DEFAULT_MAX_TOKENS` (truncate) | `truncate.ts` | 8000 | Config: `agents.defaults.tools.maxResultTokens` |
| `DEFAULT_MAX_RESULT_TOKENS` | `registry.ts` | 8000 | Same as above — deduplicate |
| `EXPIRY_TURNS` (enable_tool) | `registry.ts` | 5 | Config: `agents.defaults.tools.expiryTurns` |
| `MAX_TOOL_SIZE` (self-author) | `self-author.ts` | 8192 | Config: `agents.defaults.tools.maxToolSize` |
| `MAX_FAILURES` (self-author) | `self-author.ts` | 3 | Config: `agents.defaults.tools.maxSelfAuthorFailures` |
| `SANDBOX_TIMEOUT` (self-author) | `self-author.ts` | 10000 | Config: `agents.defaults.tools.sandboxTimeoutMs` |
| `COMMIT_TIMEOUT` | `workspace-git.ts` | 5000 | Config: `agents.defaults.tools.gitCommitTimeoutMs` |
| `MAX_MESSAGE_BYTES` (MCP) | `mcp.ts` | 102400 | Config: `gateway.mcp.maxMessageBytes` |
| `MIN_TOOL_CALLS` (skill-learner) | `skill-learner.ts` | 3 | Config: `agents.defaults.skillLearner.minToolCalls` |
| `RATE_LIMIT_MS` (skill-learner) | `skill-learner.ts` | 3600000 (1h) | Config: `agents.defaults.skillLearner.rateLimitMs` |
| `MAX_RESTART_ATTEMPTS` (signal) | `daemon.ts` | 5 | Config: `channels.signal.maxRestartAttempts` |
| `RESTART_BACKOFF_MS` (signal) | `daemon.ts` | 2000 | Config: `channels.signal.restartBackoffMs` |

**Implementation approach:** Don't add 20 new config keys immediately. Instead:

1. **Centralize**: Create `src/constants.ts` that exports all defaults with descriptive names. All modules import from there instead of defining their own.
2. **Override**: `constants.ts` reads from a `tuning` section in config if present, falls back to defaults.
3. **Document**: Each constant gets a one-line comment explaining what it controls and when you'd change it.

```yaml
# aletheia.yaml (optional, all have sane defaults)
tuning:
  watchdog:
    reAlertIntervalMs: 43200000
  tools:
    maxResultTokens: 8000
    expiryTurns: 5
  browser:
    maxPages: 3
    timeoutMs: 30000
  ephemeral:
    maxConcurrent: 3
  skillLearner:
    minToolCalls: 3
    rateLimitMs: 3600000
```

This is a `tuning` section — explicitly separated from core config — so operators know these are knobs they CAN turn but probably shouldn't without reason.

### Bootstrap File List (Low Priority — Already Fine, But Worth Noting)

The workspace file manifest is hardcoded in `bootstrap.ts`:
```typescript
{ name: "SOUL.md", priority: 1, cacheGroup: "static" },
{ name: "USER.md", priority: 2, cacheGroup: "static" },
// ... etc
```

This is actually fine — the file contract IS the system's architecture. Making this config-driven would create a footgun where agents break because someone removed `SOUL.md` from the manifest. Leave it hardcoded, but document that `_example/` is the canonical template.

### Response Quality Thresholds (Low Priority — Fine as Constants)

In `circuit-breaker.ts`:
```typescript
maxRepetitionRatio: 0.4,
minSubstanceRatio: 0.2,
maxSycophancyScore: 0.8,
```

And in `competence.ts`:
```typescript
CORRECTION_PENALTY = 0.05
SUCCESS_BONUS = 0.02
DISAGREEMENT_PENALTY = 0.01
```

These are behavioral tuning that shouldn't be operator-facing. Keep as code constants but move to `constants.ts` for centralization.

### Docker Compose (Medium Priority — Blocks Portability)

The docker-compose files hardcode volume paths:
```yaml
volumes:
  - /mnt/ssd/aletheia/data/qdrant:/qdrant/storage
```

**Fix:** Use env var substitution:
```yaml
volumes:
  - ${ALETHEIA_DATA_DIR:-./data}/qdrant:/qdrant/storage
```

`aletheia setup` writes `ALETHEIA_DATA_DIR` to an `.env` file next to the compose file. The compose file picks it up automatically.

---

## Implementation Phases (Revised After Audit)

### Phase 1: Process Lifecycle + Centralization (highest impact)
- `aletheia up` / `aletheia down` — shell script wrapping docker compose + gateway process
- PID file management in `~/.aletheia/pids/`
- Extend existing `aletheia doctor` with preflight checks (services reachable, workspaces valid, API key valid)
- Extend existing `aletheia status` with `--detail` flag for full component breakdown
- **Centralize constants:** Create `src/constants.ts`, move all magic numbers from individual modules
- **Unify service URLs:** Single memory sidecar URL resolution, eliminate `ALETHEIA_MEMORY_URL` vs `MEMORY_SIDECAR_URL` inconsistency
- **Fix `/mnt/ssd/aletheia` fallback:** Change `paths.ts` default to `process.cwd()`, docker-compose to `${ALETHEIA_DATA_DIR}`, fail explicitly if `ALETHEIA_ROOT` unset
- **Unblocks single-command start/stop and forkability immediately**

### Phase 2: Setup Wizard + Agent CRUD (onboarding path)
- `aletheia setup` interactive flow (API key, Signal, first agent, memory containers)
- `aletheia agent add/list/remove` — CLI commands that modify `aletheia.json` and scaffold workspaces
- First-turn onboarding prompt injected for new agents
- YAML config support in `taxis/loader.ts` (detect by extension, parse with `yaml` package, feed into existing Zod schema)
- Setup state persistence for idempotent re-runs

### Phase 3: Config + Profiles + Cost Guards
- Environment profiles (`--profile dev|production`) — overlay on existing config
- Per-agent token/cost budget limits in Zod schema + gateway enforcement
- Hot reload: `fs.watch` on config file → re-validate → diff agents → apply
- `env.vars` reference syntax (`${ANTHROPIC_API_KEY}`) for team-safe configs
- Migrate documentation from `.env` convention to `env.vars` in config

### Phase 4: Backup + Observability Hardening
- `aletheia backup` / `aletheia restore` — snapshot config, workspaces, Qdrant, Neo4j, sessions.db
- Automated backup scheduling via config
- Persist cost/usage metrics to SQLite (survive gateway restarts — currently in-memory)
- Daily/weekly/monthly cost aggregation API + UI charts
- `aletheia agent test <id>` — smoke test (send prompt, verify response, check tools)
- Web UI agent management panel (CRUD, leveraging existing MetricsView)

### Phase 5: Team Onboarding + Upgrade
- `secrets.yaml` / `local.yaml` overlay pattern for team repos
- `aletheia agent add` from web UI (team members self-serve)
- `aletheia upgrade` — pull, migrate config schema, rebuild, restart
- Plugin/MCP quickstart guide (document what exists, not build new)
- Shared config + personal secrets pattern documented
