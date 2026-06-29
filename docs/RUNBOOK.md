# Operational runbook

For setup and deployment, see [DEPLOYMENT.md](DEPLOYMENT.md).

## Service architecture

```text
aletheia                         (port 18789)  -- Rust binary, API
+-- signal-cli daemon            (port 8080)   -- Signal messaging (subprocess)
+-- daemon (oikonomos)           (in-process)  -- heartbeats, scheduled tasks, prosoche
```

Memory (embedded engine, candle, fjall) is embedded in the binary. No external databases or sidecars required.

## Quick health check

```bash
aletheia health          # connectivity, dependencies
aletheia status          # agent status, sessions, cron jobs
```

---

## Start procedure

## 1. Check port is free

```bash
ss -tlnp | grep 18789
# If occupied:
fuser -k 18789/tcp
```

## 2. Start the binary

```bash
aletheia
```

The binary starts the HTTP gateway, spawns nous actors, and runs the daemon. Signal is launched automatically if configured. No subcommand needed.

Or via systemd:

```bash
systemctl --user start aletheia
```

## 3. Verify

```bash
sleep 3
curl -s http://localhost:18789/api/health | jq .
```

---

## Stop procedure

```bash
systemctl --user stop aletheia
```

Or send SIGTERM / Ctrl+C to the running process. The binary shuts down gracefully.

---

## Deploy / update

```bash
# Automated (recommended):
scripts/deploy.sh                    # pull, build, stop, copy, refresh token, start, health check

# Manual:
cd <repo>
git pull origin main
cargo build --release
systemctl --user stop aletheia
mkdir -p ~/.local/bin
cp target/release/aletheia ~/.local/bin/aletheia
systemctl --user start aletheia
curl -sf http://localhost:18789/api/health | jq .
```

## Health monitoring

```bash
# One-off health check:
scripts/health-monitor.sh

# With Signal notification on failure:
scripts/health-monitor.sh --notify

# One-off prosoche heartbeat:
scripts/aletheia-heartbeat.sh

# User systemd prosoche timer (first tick after 60s, then every 5 minutes):
install -m 0755 scripts/aletheia-heartbeat.sh ~/.local/bin/aletheia-heartbeat
mkdir -p ~/.config/systemd/user
cp instance.example/services/aletheia-health.{service,timer} ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now aletheia-health.timer
systemctl --user status aletheia-health.timer
journalctl --user -u aletheia-health --since "30 minutes ago"
```

---

## Backup automation

The daemon's backup maintenance task creates local whole-instance backup sets.
Each set contains `manifest.json`, `stores/knowledge.fjall`, `stores/sessions.db`,
present runtime stores (`stores/auth.fjall`, `stores/daemon-task-state`,
`stores/cron-locks.fjall`), configuration, workspace directories, and optional
local audit/archive data when present. Configure it in
`instance/config/aletheia.toml`:

```toml
[maintenance.backup]
enabled = true
backupIntervalHours = 24
backupRetentionCount = 7
```

## Manual backup

```bash
aletheia backup create  # create an instance backup set
aletheia backup list   # list snapshots
```

## Cron setup (daily at 02:00)

```cron
0 2 * * * /usr/bin/aletheia backup create >> /var/log/aletheia-backup.log 2>&1
```

Use the built-in backup command for cron or timer automation.

## Manual restore

Backups are local instance backup sets. To inspect and verify a set:

```bash
aletheia backup verify instance/data/backups/instance/<timestamp>
```

---

## Common issues

## EADDRINUSE on port 18789

```bash
fuser 18789/tcp              # find PID
fuser -k 18789/tcp           # kill it
sleep 2
aletheia
```

## Signal-cli not receiving messages

```bash
ps aux | grep signal-cli | grep -v grep
# If not running, restart the binary -- it auto-starts signal-cli.
# If running but not receiving:
signal-cli -a +15550100001 receive --timeout 5
```

## Prosoche waking too frequently

```bash
cat <repo>/nous/<agent-id>/PROSOCHE.md
journalctl --user -u aletheia --since "1 hour ago" | grep prosoche
```

## Agent not responding

```bash
aletheia status              # check agent and session state
aletheia health              # check config and connectivity
ls -la <repo>/nous/<agent-id>/SOUL.md   # verify workspace readable
```

## Credential / OAuth token expired

```bash
# Look for auth errors in logs
journalctl --user -u aletheia --since "1 hour ago" 2>/dev/null | grep -E "401|429|expired|unauthorized"
```

Router auto-failover handles 429/5xx across providers. Expired OAuth tokens need manual replacement in `instance/config/aletheia.toml`.

---

## Log locations

| Service | Log |
|---------|-----|
| Gateway | stdout / `journalctl --user -u aletheia` |
| Signal-cli | Gateway stdout (subprocess) |

## Key paths

| Path | Purpose |
|------|---------|
| `instance/config/aletheia.toml` | Main config |
| `instance/data/sessions.db` | fjall session store (historical path name; see [DATA.md](DATA.md)) |
| `instance/data/engine/` | Knowledge graph (embedded Datalog engine) |
| `instance/nous/<id>/` | Agent workspaces |

## Pre-restart checklist

Always run `aletheia health` before restarting. Fix reported failures first - restarting with broken dependencies adds confusion.

---

## DB inspection

The session store is a binary fjall LSM-tree at `instance/data/sessions.db`. There is no ad-hoc SQL interface. See [DATA.md](DATA.md) for the storage backend authority.

### Active session count per agent

```bash
aletheia status
```

### Recent sessions with message counts

Use the HTTP API (`GET /api/v1/sessions`) or `aletheia status`.

### Token usage by model over the last 7 days

No CLI equivalent exists since the SQLite-to-fjall migration (#3446). Scrape Prometheus metrics (`aletheia_llm_input_tokens_total`, `aletheia_llm_output_tokens_total`) or query the store after stopping the service.

### Large sessions (over 50k tokens)

No CLI equivalent exists. Use the HTTP API or Prometheus metrics.

### Recent agent notes

No CLI equivalent exists.

### Distillation history

No CLI equivalent exists. Check logs:

```bash
journalctl --user -u aletheia --since "1 hour ago" | grep distill
```

### Orphaned messages (no parent session)

No CLI equivalent exists.

---

## Credential rotation

## Check current credential status

```bash
aletheia credential status
```

## OAuth token (auto-refresh)

Tokens are refreshed automatically before expiry. To force a refresh:

```bash
aletheia credential refresh
```

If refresh fails (e.g. revoked grant), re-authenticate:

1. Remove the stale credential: `rm instance/config/credentials/anthropic.json`
2. Obtain a new token from [claude.ai](https://claude.ai) or via the Anthropic console.
3. Either set `ANTHROPIC_API_KEY` in the environment, or write the JSON credential file.
4. Verify: `aletheia credential status`

## Static API key rotation

1. Generate a new key in the Anthropic console.
2. Set the environment variable `ANTHROPIC_API_KEY`, or add a declarative provider entry in `instance/config/aletheia.toml`:
   ```toml
   [[providers]]
   name = "anthropic-cloud"
   providerType = "anthropic"
   apiKeyEnv = "ANTHROPIC_API_KEY"
   deploymentTarget = "cloud"
   models = ["claude-sonnet-4-6"]
   ```
3. Restart the service: `systemctl --user restart aletheia`
4. Confirm: `aletheia health`

## Verify the new key is live

```bash
journalctl --user -u aletheia --since "1 minute ago" | grep -E "401|403|credential|auth"
# No auth errors = rotation successful
```

---

## Performance debugging

## Check current system status

```bash
aletheia status          # agent states, session counts, cron schedule
aletheia health          # LLM connectivity and cost
```

## Identify slow sessions

Sessions with high token counts can slow LLM round-trips. No CLI equivalent exists since the SQLite-to-fjall migration (#3446). Use `aletheia status` or Prometheus metrics.

Archive overloaded sessions:

```bash
curl -sf -X POST http://localhost:18789/api/v1/sessions/<id>/archive \
  -H "Authorization: Bearer <token>"
```

## Prometheus metrics

```bash
curl -sf http://localhost:18789/metrics | grep aletheia
```

Key metrics:
- `aletheia_llm_request_duration_seconds` - LLM latency distribution
- `aletheia_llm_ttft_seconds` - time-to-first-token
- `aletheia_llm_input_tokens_total` / `aletheia_llm_output_tokens_total` - throughput
- `aletheia_llm_cache_tokens_total{type="read"}` - prompt cache hit rate

## Maintenance task status

```bash
aletheia maintenance status
```

Run a specific task manually:

```bash
aletheia maintenance run trace-rotation --verbose
aletheia maintenance run drift-detection --verbose
aletheia maintenance run db-monitor --verbose
```

Drift detection compares the live instance root against the sibling
`instance.example` template. If the template directory is unavailable, the task
reports degraded/failed instead of clean.

## Log latency spikes

```bash
journalctl --user -u aletheia --since "1 hour ago" | grep -E "latency|slow|timeout|ms\b"
```

---

## Backup and restore

## Create a backup

```bash
aletheia backup create
# Writes instance/data/backups/instance/<timestamp>/
```

## List available backups

```bash
aletheia backup list
aletheia backup list --json    # machine-readable
```

## Restore from backup

Restoring requires a complete instance backup set. Always stop the service and
verify the set first. The restore command reads the manifest, stages every
selected entry, verifies the staged copy, swaps entries into place, and rolls
back automatically if publish fails.

```bash
systemctl --user stop aletheia
BACKUP=instance/data/backups/instance/<timestamp>
aletheia backup verify "$BACKUP"
aletheia backup restore "$BACKUP"
systemctl --user start aletheia
aletheia health
```

Use `--include <selector>` or `--exclude <selector>` for intentional partial
restore. Selectors match manifest names such as `sessions.db`, backup paths such
as `stores/sessions.db`, or target paths such as `data/archive`.

## Prune old backups

```bash
aletheia backup prune --keep 5    # interactive
aletheia backup prune --keep 5 --yes    # skip confirmation
```

## Export sessions as JSON (before deletion)

No built-in JSON export exists since the SQLite-to-fjall migration (#3446). Archived sessions are already JSON in `instance/data/archive/sessions/`. The `aletheia backup --export-json` command referenced in older docs and in `scripts/backup-cron.sh` is removed; do not use it.

## Verify backup integrity

```bash
aletheia backup verify instance/data/backups/instance/<timestamp>
```

---

## Log analysis

## Live log tail

```bash
journalctl --user -u aletheia -f
```

## Last hour of errors

```bash
journalctl --user -u aletheia --since "1 hour ago" --priority err..warning
```

## Search for specific patterns

```bash
# Auth / credential failures
journalctl --user -u aletheia --since "1 hour ago" | grep -E "401|403|auth|credential|expired"

# Rate limiting
journalctl --user -u aletheia --since "1 hour ago" | grep -E "429|rate.limit|retry.after"

# LLM provider errors
journalctl --user -u aletheia --since "1 hour ago" | grep -E "500|503|provider|hermeneus"

# Session activity
journalctl --user -u aletheia --since "1 hour ago" | grep -E "session|nous_id"
```

## Export logs to file

```bash
journalctl --user -u aletheia --since "24 hours ago" --output cat > /tmp/aletheia.log
```

## Log verbosity

Increase log detail at runtime by setting `RUST_LOG` before starting:

```bash
RUST_LOG=aletheia=debug aletheia
RUST_LOG=aletheia_hermeneus=trace,aletheia=info aletheia   # LLM-only trace
```

---

## Watchdog

Aletheia distinguishes two watchdog mechanisms with different operational status.

### Systemd watchdog heartbeat (live)

When Aletheia runs under a systemd unit with `Type=notify` and `WatchdogSec`,
the runtime sends `READY=1`, periodic `WATCHDOG=1`, and `STOPPING=1`
notifications through `sd_notify`. This is the live watchdog path.

This mechanism monitors the whole Aletheia service. If the Tokio runtime stops
sending heartbeats, systemd can restart the service. It does not monitor or
restart individual daemon tasks, agents, or child processes.

Check the live heartbeat with:

```bash
journalctl --user -u aletheia --since "1 hour ago" | grep "systemd watchdog"
systemctl --user show aletheia -p WatchdogUSec -p Type
```

### Daemon process watchdog (live when enabled)

When `[maintenance.watchdog].enabled = true`, each `TaskRunner` constructs a
per-task watchdog. The runner registers an in-flight task when it spawns,
unregisters it when it completes, and lets the watchdog cancel and reschedule
the task when no heartbeat arrives before `heartbeat_timeout_secs`.

Expected implementation-level log messages:

| Message | Meaning |
|---------|---------|
| `watchdog: registered process` | An in-flight daemon task is now monitored |
| `watchdog: hung process detected - no heartbeat` | The task exceeded the heartbeat timeout |
| `watchdog: restarting process` | The watchdog requested cancellation and restart |
| `watchdog: process restarted successfully` | Restart command was accepted by the runner |
| `watchdog: restart failed - applying backoff` | Restart command failed and backoff applies |
| `watchdog: max restarts exceeded - abandoning process` | The task exceeded `max_restarts` |

Config shape:

```toml
[maintenance.watchdog]
enabled = false
heartbeat_timeout_secs = 60
check_interval_secs = 10
max_restarts = 5
```

### Recovery guidance

```bash
# If the whole service is unhealthy, rely on systemd or restart it directly:
systemctl --user status aletheia
systemctl --user restart aletheia
aletheia health
```

---

## Nous roles

Every nous agent runs under a role that determines which tools it can call and which model it defaults to. Roles apply to both primary agents and ephemeral sub-agents spawned during a session.

### Role inventory

| Role | Purpose | Default model |
|------|---------|---------------|
| `Coder` | Implementation, testing, debugging | claude-sonnet-4 |
| `Researcher` | Investigation, documentation, comparison | claude-sonnet-4 |
| `Reviewer` | Code review, standards compliance, risk assessment | claude-opus-4 |
| `Explorer` | Codebase exploration, architecture understanding | claude-haiku-4-5 |
| `Runner` | Task execution, commands, deployment | claude-haiku-4-5 |

### Tool access per role

| Role | Tools |
|------|-------|
| `Coder` | `read`, `write`, `edit`, `exec`, `grep`, `find`, `ls`, `view_file`, `memory_search`, `note` |
| `Researcher` | `read`, `grep`, `find`, `ls`, `view_file`, `web_fetch`, `memory_search`, `note` |
| `Reviewer` | `read`, `grep`, `find`, `ls`, `view_file`, `memory_search` |
| `Explorer` | `read`, `grep`, `find`, `ls`, `view_file` |
| `Runner` | `read`, `exec`, `grep`, `find`, `ls`, `view_file` |

Enforcement happens twice: the tool list sent to the LLM is filtered to the allowlist, and any tool call in the response outside the allowlist is blocked before execution.

Tool-group policy is explicit and fail-closed. In agent config, `toolGroups`
accepts `"all"`, `"deny"`, or a list such as `["read", "verify"]`; absent or
empty values deny all grouped tools. In `roles.toml`, `tool_groups` uses the
same values. Use `"all"` only for admin/full-access roles with a documented
reason in the change record.

### Roles in logs

Span fields are set at spawn time. To find role-related activity:

```bash
journalctl --user -u aletheia --since "1 hour ago" | grep -E "spawn\.role|ephemeral|researcher"
```

| Message | Level | Meaning |
|---------|-------|---------|
| `ephemeral actor started` | info | Sub-agent began (includes `spawn.id`, `spawn.role`) |
| `spawning researcher` | info | Research domain spawn initiated |
| `researcher completed` | info | Success (includes input and output token counts) |
| `researcher timed out` | warn | Spawn exceeded `timeout_secs` |
| `researcher failed` | warn | Non-timeout error (includes error content) |
| `researcher task panicked` | warn | Background task panic |
| `research phase complete` | info | Summary of all domains (includes `succeeded`, `total`) |

### Debugging stuck sub-agents

Sub-agents have a mandatory `timeout_secs`. When a sub-agent hangs or exceeds its budget:

```bash
journalctl --user -u aletheia --since "1 hour ago" | grep -E "timed out|panicked|inbox full|cycle detected"
```

| Symptom | Cause | Fix |
|---------|-------|-----|
| `researcher timed out after {N}s` | Task too complex for the time budget | Increase `timeout_secs` in the spawn request |
| `ask to '{id}' timed out after 30s` | Target agent not responding | Check target agent health; restart if stuck |
| `actor '{id}' inbox full` | Sub-agent overloaded | Check `aletheia_nous_inbox_saturation_total`; wait for the inbox to drain or reduce concurrent spawns |
| `ask cycle detected: {chain}` | Circular sub-agent ask chain | Redesign the call graph to break the cycle |
| `background task limit reached` | 8 concurrent tasks active | Wait for prior tasks to finish |

Ephemeral sessions use keys prefixed with `spawn:`, `ask:`, or `dispatch:`. They skip distillation and clean up their workspace on completion.

### Denied tool calls

A role's allowlist returns a denied result, not an error, when it blocks an invocation. If an agent reports it cannot perform an action, verify the requested name appears in its role's allowlist.

---

## Dianoia planning

Dianoia manages multi-phase projects from research through verification. Each project is backed by a `PROJECT.json` file in the project workspace directory.

### Project lifecycle

Projects advance through these states in order:

```
Created → Questioning → Researching → Scoping → Planning → Discussing → Executing → Verifying → Complete
                                                                                                  Abandoned (terminal)
```

`Paused { previous }` can occur between any active states and stores the prior state for resumption.

### Inspect active projects

To view a project's current state and plan breakdown:

```bash
cat <projects-root>/<project-id>/PROJECT.json \
  | jq '{name, state, phases: [.phases[] | {name, state, plans: [.plans[] | {title, state, iterations}]}]}'
```

To find all projects in a given state:

```bash
find <projects-root> -name PROJECT.json -exec grep -l '"state":"Executing"' {} \;
```

During an active session, use `plan_status project_id=<id>` to query the current state.

### Plan states

| State | Meaning |
|-------|---------|
| `Pending` | Not yet ready to execute |
| `Ready` | Dependencies satisfied, can execute |
| `Executing` | Active |
| `Complete` | Done successfully |
| `Failed` | Execution failed |
| `Skipped` | Intentionally bypassed |
| `Stuck` | Exceeded iteration limit without completing |

### Recovering stuck plans

A plan enters `Stuck` when it exceeds `max_iterations` without completing. The stuck detector also flags patterns before that limit is reached:

| Pattern | Trigger threshold | Suggestion |
|---------|-------------------|------------|
| `RepeatedError` | Same error 3 times in a row | "Consider a different approach" |
| `SameToolSameArgs` | Same tool and args 3 times in a row | "Approach is not working" |
| `AlternatingFailure` | Two tools alternating with failures, 3 cycles | "Both approaches are failing" |
| `EscalatingRetry` | Same tool and error 3 times across a 20-invocation window | "Consider changing strategy" |

To manually recover a stuck plan in an active session:

```bash
# 1. Check current status:
plan_status project_id=<id>

# 2. Fail the stuck plan with a reason:
plan_step_fail project_id=<id> phase_id=<phase-id> plan_id=<plan-id> reason="manual: retrying with different approach"

# 3. Add a replacement plan or advance the phase.
```

### Reconciliation errors

Reconciliation runs when the database and filesystem states diverge. The outcome is classified as:

| Direction | Meaning |
|-----------|---------|
| `InSync` | Both sources agree (within 5-second tolerance) |
| `DbToFiles` | DB is newer; filesystem regenerated from DB |
| `FilesToDb` | Filesystem is newer; DB updated from filesystem |
| `DbOnly` | Project exists in DB only, no filesystem workspace |
| `FilesOnly` | Filesystem workspace exists, project not in DB |

Conflicts are logged at WARN level with the field name, the DB value, the filesystem value, and which source won.

```bash
journalctl --user -u aletheia --since "1 hour ago" | grep reconcil
```

For `FilesOnly` projects (workspace present but DB entry lost), trigger a planning session to import the workspace:

```bash
# Find orphaned workspaces:
find <projects-root> -name PROJECT.json | xargs grep -l '"state"'
```

### Blockers

Blockers are stored as Markdown files at `.dianoia/blockers/<phase-id>/<plan-id>.md` inside the project workspace. A plan with an active blocker file will not advance. Remove the file once the blocker is resolved.

---

## Melete distillation

Melete compresses conversation history into structured summaries when a session grows large. It runs as a background task after each turn completes and writes a summary message back into the session.

### Trigger conditions

Distillation fires when any of these conditions are true:

| Condition | Threshold |
|-----------|-----------|
| Context token count | >= 120,000 tokens |
| Message count | >= 150 messages |
| Session stale (no distillation in 7+ days) | >= 20 messages |
| Session never distilled | >= 30 messages |
| Context ratio (legacy path) | >= 70% of context window AND >= 10 messages |

Distillation never runs on ephemeral sessions (keys prefixed `ask:`, `spawn:`, or `dispatch:`). A 60-second idempotency guard prevents duplicate runs from concurrent background tasks.

### Check distillation history

No CLI equivalent exists since the SQLite-to-fjall migration (#3446). Check logs:

### Diagnose failures

```bash
journalctl --user -u aletheia --since "1 hour ago" | grep distill
```

| Level | Message | Meaning |
|-------|---------|---------|
| info | `triggering distillation` | Started (includes trigger reason) |
| info | `background distillation complete` | Success |
| warn | `distillation LLM call failed` | Provider error, backoff active |
| warn | `distillation produced empty summary` | LLM returned no content |
| warn | `no provider for distillation model` | Configured model unavailable |
| warn | `background task limit reached, skipping distillation` | 32 concurrent background tasks active |
| error | `context exceeds window; dropping oldest messages as last-resort fallback` | Context too large even after distillation |

### Failure backoff

After each failure, distillation skips an increasing number of turns before retrying:

| Consecutive failures | Turns skipped |
|----------------------|---------------|
| 1 | 1 |
| 2 | 2 |
| 3 | 4 |
| 4+ | 8 (maximum) |

Backoff resets on the next successful distillation.

### Token budget exceeded recovery

When context grows past the model's window and distillation has not kept up, the service drops oldest messages as a last resort. This is logged at ERROR level. Archive the overloaded session to stop the bleeding:

```bash
curl -sf -X POST http://localhost:18789/api/v1/sessions/<id>/archive \
  -H "Authorization: Bearer <token>"
```

The session history remains in the archive. Start a fresh session for new work.

### Configuration

The distillation model defaults to the workspace-wide `koina::defaults::DEFAULT_MODEL` (`claude-sonnet-4-6`; the single source of truth, see #4235). There is no `distillation_model` config field under `[agents.defaults]` in the typed config; per-agent model selection uses the `model.primary` / `model.fallbacks` fields documented in `docs/CONFIGURATION.md#agents`.

---

## Config hot-reload

Config changes take effect immediately (hot) or require a service restart (cold), depending on which fields changed.

### Hot vs cold fields

**Cold fields (require restart):**

| Prefix | What it controls |
|--------|-----------------|
| `gateway.port` | Server listen port |
| `gateway.bind` | Bind address |
| `gateway.tls.*` | TLS certificate, key, and CA |
| `gateway.auth.mode` | Authentication mode |
| `gateway.csrf` | CSRF settings |
| `gateway.bodyLimit` | Request body size limit |
| `channels.*` | Channel configuration |

All other fields are hot-reloadable, including agent defaults (`model`, `thinkingBudget`, `maxToolIterations`, `timeoutSeconds`, `contextTokens`), maintenance timers, and embedding provider settings.

### Trigger via SIGHUP

```bash
# Find the process PID:
systemctl --user show aletheia --property=MainPID --value

# Send SIGHUP:
kill -HUP <pid>
```

On receipt, the service re-reads `instance/config/aletheia.toml`, validates the new config, then swaps it in atomically. If validation fails, the current config is unchanged.

### Trigger via API

```bash
curl -sf -X POST http://localhost:18789/api/v1/config/reload \
  -H "Authorization: Bearer <token>"
```

Response:

```json
{
  "hotReloaded": 2,
  "restartRequired": ["gateway.port"],
  "changed": ["agents.defaults.model", "gateway.port"]
}
```

`hotReloaded` is the count of values applied immediately. `restartRequired` lists cold fields that changed but were not applied.

### Diagnose failed reloads

```bash
journalctl --user -u aletheia --since "10 minutes ago" | grep -E "reload|config"
```

| Level | Message | Meaning |
|-------|---------|---------|
| info | `received SIGHUP, reloading config` | Signal received |
| info | `config reload: no changes detected` | TOML unchanged, nothing applied |
| info | `config reload: value updated` | Hot field applied (includes `path`) |
| warn | `config reload: cold value changed (restart required to take effect)` | Cold field detected, not applied |
| info | `config reload complete` | Summary (includes `hot_reloaded`, `restart_required` counts) |
| error | `config reload failed, keeping current config` | Load or validation failure |

If reload fails, validate the config file directly:

```bash
aletheia check-config
```

Inspect the current live config to compare against what is on disk:

```bash
curl -sf http://localhost:18789/api/v1/config \
  -H "Authorization: Bearer <token>" | jq .
```

Common validation errors:

| Error | Fix |
|-------|-----|
| `port must be between 1 and 65535` | Set a valid port number |
| `agency must be "unrestricted", "standard", or "restricted"` | Check `agents.defaults.agency` for typos |
| `bootstrapMaxTokens must be less than contextTokens` | Reduce `bootstrapMaxTokens` or increase `contextTokens` |
| `maxToolIterations must be between 1 and 10000` | Correct the value in `[agents.defaults]` |

### Apply cold field changes

After updating a cold field, restart the service:

```bash
systemctl --user restart aletheia
aletheia health
```
