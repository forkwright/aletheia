# Aletheia Operational Runbook

Quick reference for starting, stopping, diagnosing, and recovering Aletheia services.
For initial setup and deployment, see [DEPLOYMENT.md](DEPLOYMENT.md).

## Service Architecture

```
aletheia gateway              (port 18789)  — Node.js runtime, web UI, API
├── signal-cli daemon         (port 8080)   — Signal messaging
├── prosoche daemon           (background)  — attention/wake system
├── aletheia-memory sidecar   (port 8230)   — mem0 FastAPI (Python)
│   ├── qdrant                (port 6333)   — vector store (container)
│   └── neo4j                 (port 7474/7687) — graph store (container)
└── cron jobs                 (in-process)  — heartbeats, scheduled tasks
```

## Quick Health Check

```bash
aletheia doctor          # connectivity, dependencies, boot persistence
aletheia status          # agent status, sessions, cron jobs
```

## Start Procedure

### 1. Verify containers are running

```bash
podman ps | grep -E "qdrant|neo4j"
# If not running:
podman start qdrant neo4j
```

### 2. Verify memory sidecar

```bash
curl -s http://localhost:8230/health | python3 -m json.tool
# Expected: {"status":"ok","qdrant":"ok","neo4j":"ok","embedder":"ok"}
#
# If down, check: systemctl --user status aletheia-memory
# Or start manually:
cd <repo>/infrastructure/memory/sidecar
.venv/bin/uvicorn aletheia_memory.app:app --host 0.0.0.0 --port 8230 &
```

### 3. Check port is free

```bash
ss -tlnp | grep 18789
# If occupied: find and kill the stale process
# fuser -k 18789/tcp
```

### 4. Start gateway

```bash
aletheia gateway start
# Or directly:
# node /usr/local/bin/aletheia gateway start
```

### 5. Verify

```bash
sleep 3
curl -s http://localhost:18789/api/setup/status | python3 -m json.tool
```

### 6. Start prosoche (if not running)

```bash
ps aux | grep prosoche | grep -v grep
# If not running:
cd <repo>/infrastructure/prosoche
.venv/bin/python3 -m prosoche.daemon &
```

## Stop Procedure

```bash
# Gateway
pkill -f "aletheia gateway" || pkill -f "entry.mjs"

# Prosoche
pkill -f "prosoche.daemon"

# Memory sidecar (optional — usually leave running)
pkill -f "aletheia_memory"

# Containers (optional — usually leave running)
podman stop qdrant neo4j
```

## Deploy / Update

```bash
cd <repo>

# 1. Pull latest
git pull origin main

# 2. Build runtime
cd infrastructure/runtime && npx tsdown
cd ../..

# 3. Build UI (if UI changes)
cd ui && npm run build
cd ..

# 4. Restart gateway
pkill -f "aletheia gateway"
sleep 2
aletheia gateway start
```

## Common Issues

### EADDRINUSE on port 18789

Port still held by a previous process.

```bash
fuser 18789/tcp              # find PID
fuser -k 18789/tcp           # kill it
sleep 2
aletheia gateway start
```

### Memory sidecar unhealthy

```bash
curl -s http://localhost:8230/health
# Check individual services:
curl -s http://localhost:6333/healthz       # Qdrant
curl -s http://localhost:7474               # Neo4j (browser)

# Restart sidecar (VOYAGE_API_KEY only available in systemd service file):
systemctl --user restart aletheia-memory
```

### Signal-cli not receiving messages

```bash
ps aux | grep signal-cli | grep -v grep
# If not running, gateway auto-starts it. Restart gateway.
# If running but not receiving:
signal-cli -a +15124288605 receive --timeout 5
```

### Prosoche waking too frequently

Check dedup window and fingerprint:

```bash
# View current prosoche state
cat <repo>/nous/syn/PROSOCHE.md

# Check daemon logs
journalctl --user -u prosoche --since "1 hour ago" 2>/dev/null || \
  tail -50 /tmp/prosoche.log
```

### Agent not responding

```bash
# Check if session exists
aletheia sessions

# Check agent config
aletheia doctor

# Verify workspace is readable
ls -la <repo>/nous/<agent-id>/SOUL.md
```

### Credential / OAuth token expired

Tokens expire and must be replaced manually. Check:

```bash
# Look for 401/429 in logs
journalctl --user -u aletheia --since "1 hour ago" 2>/dev/null | grep -E "401|429|expired|unauthorized"

# Config location for credentials:
cat ~/.aletheia/aletheia.json | python3 -c "import json,sys; c=json.load(sys.stdin); [print(k) for k in c.get('models',{}).get('providers',{}).keys()]"
```

Router auto-failover handles 429/5xx across configured providers, but expired OAuth tokens need manual replacement in `~/.aletheia/aletheia.json`.

### NAS SSH refused

```bash
ping -c 1 <NAS_IP>           # Should succeed (NAS is up)
ssh nas                      # Port 22 refused = SSH service disabled in Synology DSM
# Fix: Enable SSH in DSM → Control Panel → Terminal & SNMP → Enable SSH
```

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
| `~/.aletheia/aletheia.json` | Main config |
| `~/.aletheia/sessions.db` | SQLite session store |
| `<repo>/` | Monorepo root |
| `<repo>/nous/<id>/` | Agent workspaces |
| `<repo>/infrastructure/runtime/` | TypeScript runtime |
| `<repo>/infrastructure/memory/sidecar/` | Python memory sidecar |
| `<repo>/infrastructure/prosoche/` | Prosoche daemon |
| `<repo>/ui/` | Svelte web UI |

## Validation Before Restart

**Always** run `aletheia doctor` before restarting. If it reports failures, fix those first — restarting with broken dependencies just adds confusion.
