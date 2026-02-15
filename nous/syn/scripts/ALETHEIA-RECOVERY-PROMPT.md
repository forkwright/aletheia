# Aletheia Recovery Prompt

If the gateway won't start, use this with Claude Code or manually.

---

## Key Paths

| What | Path |
|------|------|
| Root | `/mnt/ssd/aletheia/` |
| Nous workspaces | `/mnt/ssd/aletheia/nous/{syn,eiron,demiurge,syl,arbor,akron}` |
| Runtime config | `/home/syn/.aletheia/aletheia.json` |
| Service | `/etc/systemd/system/aletheia.service` |
| Gateway binary | `/usr/local/bin/aletheia` → `/mnt/ssd/aletheia/infrastructure/runtime/aletheia.mjs` |
| Logs | `/tmp/aletheia/aletheia-YYYY-MM-DD.log` |

---

## Recovery Steps

### 1. SSH to server
```bash
ssh server   # or: ssh codykickertz@192.168.0.29
```

### 2. Check service status
```bash
sudo systemctl status aletheia
sudo journalctl -u aletheia -n 50 --no-pager
```

### 3. Common issues

**Zombie gateway blocking port:**
```bash
# Check if old gateway still holds port 18789
ss -tlnp | grep 18789
# Kill it if needed
sudo kill <PID>
sudo systemctl restart aletheia
```

**Config syntax error:**
```bash
# Validate JSON
python3 -c "import json; json.load(open('/home/syn/.aletheia/aletheia.json'))"
```

**ACL permissions on shared/bin:**
```bash
# If agents get EACCES on tools:
sudo setfacl -m u:syn:rwx /mnt/ssd/aletheia/shared/bin/*
```

**Signal-cli not responding:**
```bash
curl -s http://127.0.0.1:8080/v1/about
# If dead, restart aletheia (it spawns signal-cli as child)
sudo systemctl restart aletheia
```

### 4. Validate and restart
```bash
aletheia doctor
sudo systemctl restart aletheia
journalctl -u aletheia -f
```

### 5. Verify Signal works
Send a test message to any group. Check logs for delivery.

---

## Supporting Services

| Service | Command | Port |
|---------|---------|------|
| aletheia | `systemctl status aletheia` | 18789 |
| aletheia-memory | `systemctl status aletheia-memory` | 8230 |
| aletheia-prosoche | `systemctl status aletheia-prosoche` | — |
| docker (qdrant, neo4j, langfuse) | `docker ps` | 6333, 7687, 3100 |

---

*Updated: 2026-02-14*
