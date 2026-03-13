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

### 1. Check port is free

```bash
ss -tlnp | grep 18789
# If occupied:
fuser -k 18789/tcp
```

### 2. Start the binary

```bash
aletheia
```

The binary serves the HTTP gateway, spawns nous actors, starts the daemon, and (if configured) launches signal-cli. No subcommand needed.

Or via systemd:

```bash
systemctl --user start aletheia
```

### 3. Verify

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
cd <repo>
git pull origin main
cargo build --release
systemctl --user restart aletheia
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
