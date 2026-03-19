# Operational runbook

For setup and deployment, see [DEPLOYMENT.md](DEPLOYMENT.md).

## Service architecture

```text
aletheia                         (port 18789)  -- Rust binary, API
+-- signal-cli daemon            (port 8080)   -- Signal messaging (subprocess)
+-- daemon (oikonomos)           (in-process)  -- heartbeats, scheduled tasks, prosoche
```

Memory (embedded engine, candle, SQLite) is embedded in the binary. No external databases or sidecars required.

## Quick health check

```bash
aletheia health          # connectivity, dependencies
aletheia status          # agent status, sessions, cron jobs
```

---

## Start procedure

### 1. check port is free

```bash
ss -tlnp | grep 18789
# If occupied:
fuser -k 18789/tcp
```

### 2. start the binary

```bash
aletheia
```

The binary serves the HTTP gateway, spawns nous actors, starts the daemon, and (if configured) launches signal-cli. No subcommand needed.

Or via systemd:

```bash
systemctl --user start aletheia
```

### 3. verify

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
cp target/release/aletheia ~/ergon/bin/aletheia
systemctl --user start aletheia
curl -sf http://localhost:18789/api/health | jq .
```

## Health monitoring

```bash
# One-off check:
scripts/health-monitor.sh

# With Signal notification on failure:
scripts/health-monitor.sh --notify

# Systemd timer (every 5 minutes):
cp instance.example/services/aletheia-health.{service,timer} ~/.config/systemd/user/
systemctl --user enable --now aletheia-health.timer
```

---

## Common issues

### EADDRINUSE on port 18789

```bash
fuser 18789/tcp              # find PID
fuser -k 18789/tcp           # kill it
sleep 2
aletheia
```

### Signal-cli not receiving messages

```bash
ps aux | grep signal-cli | grep -v grep
# If not running, restart the binary -- it auto-starts signal-cli.
# If running but not receiving:
signal-cli -a +15550100001 receive --timeout 5
```

### Prosoche waking too frequently

```bash
cat <repo>/nous/<agent-id>/PROSOCHE.md
journalctl --user -u aletheia --since "1 hour ago" | grep prosoche
```

### Agent not responding

```bash
aletheia status              # check agent and session state
aletheia health              # check config and connectivity
ls -la <repo>/nous/<agent-id>/SOUL.md   # verify workspace readable
```

### Credential / OAuth token expired

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
| `instance/data/sessions.db` | SQLite session store |
| `instance/data/engine/` | Knowledge graph (embedded Datalog engine) |
| `instance/nous/<id>/` | Agent workspaces |

## Pre-restart checklist

Always run `aletheia health` before restarting. Fix reported failures first - restarting with broken dependencies adds confusion.

---

## DB inspection queries

The session store is a SQLite database at `instance/data/sessions.db`.

```bash
sqlite3 instance/data/sessions.db
```

### Active session count per agent

```sql
SELECT nous_id, COUNT(*) AS active_sessions
FROM sessions
WHERE status = 'active'
GROUP BY nous_id;
```

### Recent sessions with message counts

```sql
SELECT id, nous_id, status, message_count, token_count_estimate, created_at
FROM sessions
ORDER BY created_at DESC
LIMIT 20;
```

### Token usage by model over the last 7 days

```sql
SELECT model,
       SUM(input_tokens)       AS total_input,
       SUM(output_tokens)      AS total_output,
       SUM(cache_read_tokens)  AS cache_hits,
       SUM(cache_write_tokens) AS cache_writes,
       COUNT(*)                AS turns
FROM usage
WHERE created_at >= datetime('now', '-7 days')
GROUP BY model
ORDER BY total_output DESC;
```

### Large sessions (over 50k tokens)

```sql
SELECT id, nous_id, token_count_estimate, message_count, status, created_at
FROM sessions
WHERE token_count_estimate > 50000
ORDER BY token_count_estimate DESC;
```

### Recent agent notes

```sql
SELECT n.nous_id, n.category, n.content, n.created_at
FROM agent_notes n
ORDER BY n.created_at DESC
LIMIT 20;
```

### Distillation history

```sql
SELECT session_id, messages_before, messages_after,
       tokens_before, tokens_after, facts_extracted, model, created_at
FROM distillations
ORDER BY created_at DESC
LIMIT 10;
```

### Orphaned messages (no parent session)

```sql
SELECT COUNT(*) AS orphan_count
FROM messages m
LEFT JOIN sessions s ON s.id = m.session_id
WHERE s.id IS NULL;
```

---

## Credential rotation

### Check current credential status

```bash
aletheia credential status
```

### OAuth token (auto-refresh)

Tokens are refreshed automatically before expiry. To force a refresh:

```bash
aletheia credential refresh
```

If refresh fails (e.g. revoked grant), re-authenticate:

1. Remove the stale credential: `rm instance/config/credentials/anthropic.json`
2. Obtain a new token from [claude.ai](https://claude.ai) or via the Anthropic console.
3. Either set `ANTHROPIC_API_KEY` in the environment, or write the JSON credential file.
4. Verify: `aletheia credential status`

### Static API key rotation

1. Generate a new key in the Anthropic console.
2. Update `instance/config/aletheia.toml`:
   ```toml
   [provider]
   api_key = "sk-ant-..."
   ```
   Or set the environment variable `ANTHROPIC_API_KEY`.
3. Restart the service: `systemctl --user restart aletheia`
4. Confirm: `aletheia health`

### Verify the new key is live

```bash
journalctl --user -u aletheia --since "1 minute ago" | grep -E "401|403|credential|auth"
# No auth errors = rotation successful
```

---

## Performance debugging

### Check current system status

```bash
aletheia status          # agent states, session counts, cron schedule
aletheia health          # LLM connectivity and cost
```

### Identify slow sessions

Sessions with high token counts can slow LLM round-trips. Find them:

```sql
-- In sqlite3 instance/data/sessions.db
SELECT id, nous_id, token_count_estimate, message_count, status
FROM sessions
WHERE status = 'active' AND token_count_estimate > 30000
ORDER BY token_count_estimate DESC;
```

Archive overloaded sessions:

```bash
curl -sf -X POST http://localhost:18789/api/v1/sessions/<id>/archive \
  -H "Authorization: Bearer <token>"
```

### Prometheus metrics

```bash
curl -sf http://localhost:18789/metrics | grep aletheia
```

Key metrics:
- `aletheia_llm_request_duration_seconds` — LLM latency distribution
- `aletheia_llm_ttft_seconds` — time-to-first-token
- `aletheia_llm_input_tokens_total` / `aletheia_llm_output_tokens_total` — throughput
- `aletheia_llm_cache_tokens_total{type="read"}` — prompt cache hit rate

### Maintenance task status

```bash
aletheia maintenance status
```

Run a specific task manually:

```bash
aletheia maintenance run trace-rotation --verbose
aletheia maintenance run drift-detection --verbose
aletheia maintenance run db-monitor --verbose
```

### Log latency spikes

```bash
journalctl --user -u aletheia --since "1 hour ago" | grep -E "latency|slow|timeout|ms\b"
```

---

## Backup and restore

### Create a backup

```bash
aletheia backup
# Writes instance/data/backups/sessions_<timestamp>.db
```

### List available backups

```bash
aletheia backup --list
aletheia backup --list --json    # machine-readable
```

### Restore from backup

The backup is a complete SQLite copy. To restore:

```bash
systemctl --user stop aletheia
cp instance/data/backups/sessions_<timestamp>.db instance/data/sessions.db
systemctl --user start aletheia
aletheia health
```

### Prune old backups

```bash
aletheia backup --prune --keep 5    # interactive
aletheia backup --prune --keep 5 --yes    # skip confirmation
```

### Export sessions as JSON (before deletion)

```bash
aletheia backup --export-json
# Writes to instance/data/archive/sessions/
```

### Verify backup integrity

```bash
sqlite3 instance/data/backups/sessions_<timestamp>.db "PRAGMA integrity_check;"
sqlite3 instance/data/backups/sessions_<timestamp>.db "SELECT COUNT(*) FROM sessions;"
```

---

## Log analysis

### Live log tail

```bash
journalctl --user -u aletheia -f
```

### Last hour of errors

```bash
journalctl --user -u aletheia --since "1 hour ago" --priority err..warning
```

### Search for specific patterns

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

### Export logs to file

```bash
journalctl --user -u aletheia --since "24 hours ago" --output cat > /tmp/aletheia.log
```

### Log verbosity

Increase log detail at runtime by setting `RUST_LOG` before starting:

```bash
RUST_LOG=aletheia=debug aletheia
RUST_LOG=aletheia_hermeneus=trace,aletheia=info aletheia   # LLM-only trace
```
