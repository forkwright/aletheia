# OpenClaw Recovery Prompt for Metis Claude Code

If Syn doesn't come back after the filesystem reorganization, use this prompt with Claude Code on Metis (ssh syn@100.117.8.41 or local).

---

## Context

A filesystem reorganization was in progress on the moltbot server (192.168.0.29). The migration script was run but OpenClaw may have failed to restart.

**What was done:**
1. Created `/mnt/ssd/moltbot/projects/` - moved all project content from dianoia
2. Created `/mnt/ssd/moltbot/agents/` - moved all agent workspaces
3. Renamed `clawd` → `agents/syn`
4. Created `/mnt/ssd/moltbot/infrastructure/` - moved repos, signal-cli, data
5. Created symlink `clawd` → `agents/syn` for backwards compatibility

**New structure:**
```
moltbot/
├── projects/      (vehicle, craft, mba, work, career, etc.)
├── agents/        (syn, chiron, eiron, demiurge, syl, arbor, akron)
├── shared/        (unchanged)
├── infrastructure/
└── archive/
```

---

## Recovery Steps

### 1. SSH to server
```bash
ssh syn@192.168.0.29
# or via Tailscale: ssh syn@100.87.6.45
```

### 2. Check OpenClaw status
```bash
systemctl status autarkia
journalctl -u autarkia -n 50
```

### 3. If config is the problem, fix it
The config file is at: `/home/syn/.openclaw/openclaw.json`

**Key changes needed:**
- All workspace paths: `/mnt/ssd/moltbot/X` → `/mnt/ssd/moltbot/agents/X`
- Syn's workspace: `/mnt/ssd/moltbot/clawd` → `/mnt/ssd/moltbot/agents/syn`

```bash
# View current config
cat /home/syn/.openclaw/openclaw.json | jq '.agents'

# Edit config
nano /home/syn/.openclaw/openclaw.json
# OR use the patch file:
# /mnt/ssd/moltbot/agents/syn/scripts/new-config-patch.json
```

### 4. Validate and restart
```bash
openclaw doctor
sudo systemctl restart autarkia
systemctl status autarkia
```

### 5. If filesystem is broken, check symlinks
```bash
# The symlink should exist
ls -la /mnt/ssd/moltbot/clawd  # Should point to agents/syn

# If missing, create it
cd /mnt/ssd/moltbot
ln -s agents/syn clawd
```

### 6. Update script paths
If scripts are failing due to old paths:
```bash
chmod +x /mnt/ssd/moltbot/agents/syn/scripts/update-script-paths.sh
/mnt/ssd/moltbot/agents/syn/scripts/update-script-paths.sh
```

---

## Full Config Patch (if needed)

Replace the agents section in `/home/syn/.openclaw/openclaw.json` with:

```json
"agents": {
  "defaults": {
    "workspace": "/mnt/ssd/moltbot/agents/syn",
    ... (rest unchanged)
  },
  "list": [
    {"id": "main", "workspace": "/mnt/ssd/moltbot/agents/syn", ...},
    {"id": "syl", "workspace": "/mnt/ssd/moltbot/agents/syl", ...},
    {"id": "chiron", "workspace": "/mnt/ssd/moltbot/agents/chiron", ...},
    {"id": "eiron", "workspace": "/mnt/ssd/moltbot/agents/eiron", ...},
    {"id": "demiurge", "workspace": "/mnt/ssd/moltbot/agents/demiurge", ...},
    {"id": "akron", "workspace": "/mnt/ssd/moltbot/agents/akron", ...},
    {"id": "arbor", "workspace": "/mnt/ssd/moltbot/agents/arbor", ...}
  ]
}
```

The full patch is at: `/mnt/ssd/moltbot/agents/syn/scripts/new-config-patch.json`

---

## Rollback (if needed)

The symlink provides compatibility. If you need to fully rollback:

```bash
cd /mnt/ssd/moltbot

# Remove symlink
rm clawd

# Move syn back to clawd
mv agents/syn clawd

# Update config back to clawd paths
nano /home/syn/.openclaw/openclaw.json

# Restart
sudo systemctl restart autarkia
```

---

## Contact

If Cody needs to debug manually, the key files are:
- Config: `/home/syn/.openclaw/openclaw.json`
- Service: `systemctl status autarkia`
- Logs: `journalctl -u autarkia -f`
- Migration plan: `/mnt/ssd/moltbot/agents/syn/docs/filesystem-reorganization-complete.md`
