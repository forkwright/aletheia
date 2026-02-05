# Aletheia Recovery Prompt

If Syn doesn't come back after the service transition, use this with Claude Code.

---

## Context

The moltbot system was renamed to Aletheia:
- `/mnt/ssd/moltbot` → `/mnt/ssd/aletheia`
- `agents/` → `nous/`
- Service: `autarkia` → `aletheia`
- Config paths updated

---

## Current State

**Filesystem:** ✓ Complete
- `/mnt/ssd/aletheia/` exists
- `/mnt/ssd/aletheia/nous/` contains all 7 minds
- Syncthing syncing to Metis `/home/ck/aletheia/`

**Service:** Pending
- New service file: `/etc/systemd/system/aletheia.service`
- Old service still running: `autarkia`

**Config:** Pending  
- Config patch ready at: `/mnt/ssd/aletheia/nous/syn/scripts/aletheia-config-patch.json`
- Needs to be applied to: `~/.openclaw/openclaw.json`

---

## Recovery Steps

### 1. SSH to server
```bash
ssh syn@192.168.0.29
# or: ssh syn@100.87.6.45  (Tailscale)
```

### 2. Check what's running
```bash
systemctl status autarkia
systemctl status aletheia
```

### 3. If autarkia is still running and broken

Stop it and switch to aletheia:
```bash
sudo systemctl stop autarkia
sudo systemctl disable autarkia
sudo systemctl enable aletheia
sudo systemctl start aletheia
```

### 4. If config is the problem

The config needs paths updated. Apply the patch:
```bash
# Read current config
cat ~/.openclaw/openclaw.json | jq '.agents.list[].workspace'

# Should show /mnt/ssd/aletheia/nous/X paths
# If still showing /mnt/ssd/moltbot or /agents/, fix manually:

# Option A: Use openclaw CLI
openclaw gateway config.patch --raw "$(cat /mnt/ssd/aletheia/nous/syn/scripts/aletheia-config-patch.json)"

# Option B: Edit directly
nano ~/.openclaw/openclaw.json
# Change all:
#   /mnt/ssd/moltbot → /mnt/ssd/aletheia
#   /agents/ → /nous/
```

### 5. Validate and restart
```bash
openclaw doctor
sudo systemctl restart aletheia
systemctl status aletheia
journalctl -u aletheia -f
```

### 6. Verify Signal works
Send a test message. Check logs for delivery.

---

## Key Paths

| What | Path |
|------|------|
| Root | `/mnt/ssd/aletheia/` |
| Syn workspace | `/mnt/ssd/aletheia/nous/syn/` |
| Config | `~/.openclaw/openclaw.json` |
| Config link | `~/.aletheia/config.json` (symlink) |
| Service | `/etc/systemd/system/aletheia.service` |
| Old service | `/etc/systemd/system/autarkia.service` |
| Config patch | `/mnt/ssd/aletheia/nous/syn/scripts/aletheia-config-patch.json` |

---

## Full Config Patch

If needed, here are the key changes for `~/.openclaw/openclaw.json`:

```json
{
  "env": {
    "PATH": "/mnt/ssd/aletheia/shared/bin:/usr/local/bin:/usr/bin:/bin"
  },
  "agents": {
    "defaults": {
      "workspace": "/mnt/ssd/aletheia/nous/syn"
    },
    "list": [
      {"id": "main", "workspace": "/mnt/ssd/aletheia/nous/syn", ...},
      {"id": "syl", "workspace": "/mnt/ssd/aletheia/nous/syl", ...},
      {"id": "chiron", "workspace": "/mnt/ssd/aletheia/nous/chiron", ...},
      {"id": "eiron", "workspace": "/mnt/ssd/aletheia/nous/eiron", ...},
      {"id": "demiurge", "workspace": "/mnt/ssd/aletheia/nous/demiurge", ...},
      {"id": "akron", "workspace": "/mnt/ssd/aletheia/nous/akron", ...},
      {"id": "arbor", "workspace": "/mnt/ssd/aletheia/nous/arbor", ...}
    ]
  }
}
```

---

## Rollback

If everything is broken:

```bash
# Rename back
sudo mv /mnt/ssd/aletheia /mnt/ssd/moltbot
sudo mv /mnt/ssd/moltbot/nous /mnt/ssd/moltbot/agents

# Fix config (change aletheia→moltbot, nous→agents)
nano ~/.openclaw/openclaw.json

# Use old service
sudo systemctl stop aletheia
sudo systemctl start autarkia
```

---

*Created: 2026-02-05*
