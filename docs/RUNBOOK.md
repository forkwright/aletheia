# Operational Runbook

For setup and deployment, see [DEPLOYMENT.md](DEPLOYMENT.md).

## Service Architecture

```
aletheia gateway              (port 18789)  -- Node.js runtime, web UI, API
+-- signal-cli daemon         (port 8080)   -- Signal messaging
+-- prosoche daemon           (background)  -- attention/wake system
+-- aletheia-memory sidecar   (port 8230)   -- mem0 FastAPI (Python)
|   +-- qdrant                (port 6333)   -- vector store (container)
|   +-- neo4j                 (port 7474/7687) -- graph store (container)
+-- cron jobs                 (in-process)  -- heartbeats, scheduled tasks
```

## Quick Health Check

```bash
aletheia doctor          # connectivity, dependencies, boot persistence
aletheia status          # agent status, sessions, cron jobs
```

---

## Start Procedure

### 1. Verify containers

```bash
podman ps | grep -E "qdrant|neo4j"
# If not running:
podman start qdrant neo4j
```

### 2. Verify memory sidecar

```bash
curl -s http://localhost:8230/health | python3 -m json.tool
# Expected: {"status":"ok","qdrant":"ok","neo4j":"ok","embedder":"ok"}
```

If down:

```bash
systemctl --user status aletheia-memory
# Or start manually:
cd <repo>/infrastructure/memory/sidecar
.venv/bin/uvicorn aletheia_memory.app:app --host 0.0.0.0 --port 8230 &
```

### 3. Check port is free

```bash
ss -tlnp | grep 18789
# If occupied:
fuser -k 18789/tcp
```

### 4. Start gateway

```bash
aletheia gateway start
```

### 5. Verify

```bash
sleep 3
curl -s http://localhost:18789/api/setup/status | python3 -m json.tool
```

### 6. Start prosoche (if needed)

```bash
ps aux | grep prosoche | grep -v grep
# If not running:
cd <repo>/infrastructure/prosoche
.venv/bin/python3 -m prosoche.daemon &
```

---

## Stop Procedure

```bash
# Gateway
pkill -f "aletheia gateway" || pkill -f "entry.mjs"

# Prosoche
pkill -f "prosoche.daemon"

# Memory sidecar (optional -- usually leave running)
pkill -f "aletheia_memory"

# Containers (optional -- usually leave running)
podman stop qdrant neo4j
```

---

## Deploy / Update

```bash
cd <repo>
git pull origin main
cd infrastructure/runtime && npx tsdown && cd ../..
cd ui && npm run build && cd ..
pkill -f "aletheia gateway"
sleep 2
aletheia gateway start
```

---

## Common Issues

### EADDRINUSE on port 18789

```bash
fuser 18789/tcp              # find PID
fuser -k 18789/tcp           # kill it
sleep 2
aletheia gateway start
```

### Memory sidecar unhealthy

```bash
curl -s http://localhost:8230/health
curl -s http://localhost:6333/healthz       # Qdrant
curl -s http://localhost:7474               # Neo4j

# Restart (VOYAGE_API_KEY only in systemd service file):
systemctl --user restart aletheia-memory
```

### Signal-cli not receiving messages

```bash
ps aux | grep signal-cli | grep -v grep
# If not running, restart gateway -- it auto-starts signal-cli.
# If running but not receiving:
signal-cli -a +15550100001 receive --timeout 5
```

### Prosoche waking too frequently

```bash
cat <repo>/nous/syn/PROSOCHE.md
journalctl --user -u prosoche --since "1 hour ago" 2>/dev/null || \
  tail -50 /tmp/prosoche.log
```

### Agent not responding

```bash
aletheia sessions             # check session exists
aletheia doctor               # check agent config
ls -la <repo>/nous/<agent-id>/SOUL.md   # verify workspace readable
```

### Credential / OAuth token expired

```bash
# Look for auth errors in logs
journalctl --user -u aletheia --since "1 hour ago" 2>/dev/null | grep -E "401|429|expired|unauthorized"
```

Router auto-failover handles 429/5xx across providers. Expired OAuth tokens need manual replacement in `instance/config/aletheia.yaml`.

---

## Log Locations

| Service | Log |
|---------|-----|
| Gateway | stdout / `journalctl --user -u aletheia` |
| Prosoche | `/tmp/prosoche.log` or `journalctl --user -u prosoche` |
| Memory sidecar | stdout / `journalctl --user -u aletheia-memory` |
| Qdrant | `podman logs qdrant` |
| Neo4j | `podman logs neo4j` |
| Signal-cli | Gateway stdout (subprocess) |

## Key Paths

| Path | Purpose |
|------|---------|
| `instance/config/aletheia.yaml` | Main config |
| `instance/data/sessions.db` | SQLite session store |
| `instance/nous/<id>/` | Agent workspaces |
| `infrastructure/runtime/` | TypeScript runtime |
| `infrastructure/memory/sidecar/` | Python memory sidecar |
| `infrastructure/prosoche/` | Prosoche daemon |
| `ui/` | Svelte web UI |

## Pre-Restart Checklist

Always run `aletheia doctor` before restarting. Fix reported failures first - restarting with broken dependencies adds confusion.
