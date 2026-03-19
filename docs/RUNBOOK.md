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

## Backup automation

`scripts/backup-cron.sh` exports the session store to JSON and prunes old copies.

```bash
# One-off backup (keeps 7 copies, writes to $ALETHEIA_ROOT/backups/):
scripts/backup-cron.sh

# Keep 14 copies in a custom directory:
scripts/backup-cron.sh --keep 14 --output-dir /mnt/backup/aletheia
```

### Cron setup (daily at 02:00)

```cron
0 2 * * * /path/to/scripts/backup-cron.sh >> /var/log/aletheia-backup.log 2>&1
```

### Environment overrides

| Variable | Default | Purpose |
|----------|---------|---------|
| `ALETHEIA_ROOT` | `~/ergon/instance` | Instance root |
| `ALETHEIA_BINARY` | `~/ergon/bin/aletheia` | Binary path |
| `BACKUP_KEEP` | `7` | Number of backup files to retain |
| `BACKUP_OUTPUT_DIR` | `$ALETHEIA_ROOT/backups` | Backup output directory |

The script uses `flock` to prevent concurrent runs. Backup files are named `sessions-<timestamp>.json`.

### Manual restore

Backup files are plain JSON exported by `aletheia backup --export-json`. To inspect:

```bash
jq '.sessions | length' ~/ergon/instance/backups/sessions-*.json | tail -1
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
