# Disaster Recovery Runbook

Operational recovery procedures for Aletheia after service failure, data corruption, machine loss, or credential compromise.

For day-to-day operations, see [RUNBOOK.md](RUNBOOK.md). Deployment details are in [DEPLOYMENT.md](DEPLOYMENT.md). The storage backend reference is [DATA.md](DATA.md).

---

## Recovery targets

| Target | Value | Basis |
|--------|-------|-------|
| **RTO** | 30–60 minutes | Binary deploy + health check is ~30–45 s; reinstall + full NAS restore dominates the window |
| **RPO** | 24 hours | Default fjall backup interval (`interval_hours = 24`, see `FjallBackupConfig`) |

> If you need tighter RPO, reduce `interval_hours` in config or run `aletheia backup` more frequently.

---

## Scenario 1: Service crash → restart recovery

**Symptom:** Process exited, health endpoint down, systemd reports `failed`.

### Recovery sequence

```bash
# 1. Inspect last errors
journalctl --user -u aletheia --since "10 minutes ago" --priority err..warning

# 2. Restart the service
systemctl --user daemon-reload
systemctl --user restart aletheia

# 3. Wait for health endpoint (deploy script uses 30 s timeout)
sleep 5
curl -sf --max-time 5 http://localhost:18789/api/health | jq .

# 4. If restart loops, check for port conflict
ss -tlnp | grep 18789
fuser -k 18789/tcp   # only if the PID is not aletheia
```

### If restart still fails

- Roll back to the previous binary: `scripts/deploy.sh --rollback`
- Verify: `aletheia health`

---

## Scenario 2: DB corruption → restore from fjall backup

**Symptom:** `aletheia health` reports session-store or knowledge-store failures; the store fails to open or returns read errors.

### Recovery sequence

```bash
# 1. Stop the service
systemctl --user stop aletheia

# 2. List available fjall backups
aletheia backup --list

# 3. Identify the most recent good backup
LATEST=$(aletheia backup --list --json | jq -r '.[0].name')
echo "Restoring from: $LATEST"

# 4. Move corrupted store aside (do not delete until recovery is confirmed)
mv "${ALETHEIA_ROOT:-$HOME/aletheia/instance}/data/knowledge.fjall" \
   "${ALETHEIA_ROOT:-$HOME/aletheia/instance}/data/knowledge.fjall.corrupt.$(date -u +%Y%m%dT%H%M%SZ)"

# 5. Restore from the fjall backup snapshot
cp -a "${ALETHEIA_ROOT:-$HOME/aletheia/instance}/data/backups/fjall/${LATEST}" \
      "${ALETHEIA_ROOT:-$HOME/aletheia/instance}/data/knowledge.fjall"

# 6. Start and verify
systemctl --user start aletheia
aletheia health
```

### Session-store corruption

`sessions.db` is also a fjall LSM-tree. There is no built-in backup for the session store; the `aletheia backup` command only covers `knowledge.fjall`. If the session store is corrupt, your options are:

1. Restore from a filesystem snapshot (restic, ZFS, etc.) taken while the service was stopped.
2. Delete the directory and let the service recreate it on startup. Session history will be lost.

```bash
systemctl --user stop aletheia
mv "${ALETHEIA_ROOT:-$HOME/aletheia/instance}/data/sessions.db" \
   "${ALETHEIA_ROOT:-$HOME/aletheia/instance}/data/sessions.db.corrupt.$(date -u +%Y%m%dT%H%M%SZ)"
systemctl --user start aletheia
aletheia health
```

> The daemon's `FjallBackup` task only backs up `knowledge.fjall`. `scripts/backup-cron.sh` is legacy and references a removed CLI flag.

---

## Scenario 3: Machine loss → reinstall + restore from NAS backup (restic)

**Symptom:** Hardware failure, OS reinstall, or total machine loss.

### Recovery sequence

```bash
# 1. Install prerequisites (same as first-time deploy)
#    Rust 1.94+, cargo, systemctl, restic

# 2. Clone the repository and build (or download a release binary)
git clone <repo-url> aletheia
cd aletheia
cargo build --release -p aletheia

# 3. Restore instance data from a restic backup
#    (adjust RESTIC_REPOSITORY and RESTIC_PASSWORD_FILE to your environment)
export RESTIC_REPOSITORY="/path/to/restic-repo"   # or your repo URL
export RESTIC_PASSWORD_FILE="$HOME/.config/restic/password"

# Restore the latest snapshot for your host
restic restore latest --target "$HOME/aletheia-restored" \
  --include "$(hostname)/home/*/aletheia/instance" \
  --include "$(hostname)/home/*/.config/systemd/user/aletheia.service"

# 4. Recreate the instance directory from the restored snapshot
mkdir -p "$HOME/aletheia/instance"
cp -a "$HOME/aletheia-restored"/*/home/*/aletheia/instance/* "$HOME/aletheia/instance/"

# 5. Install the binary
mkdir -p "$HOME/.local/bin"
cp target/release/aletheia "$HOME/.local/bin/aletheia"

# 6. Reinstall the systemd unit
mkdir -p "$HOME/.config/systemd/user"
cp instance.example/services/aletheia.service "$HOME/.config/systemd/user/aletheia.service"
# Edit paths in the unit file if your layout differs from the defaults.

# 7. Start and verify
systemctl --user daemon-reload
systemctl --user enable --now aletheia
loginctl enable-linger
aletheia health
```

> In a reference deployment, daily restic backups can be driven by a user-level backup script. Adapt the `RESTIC_REPOSITORY` and snapshot paths to your own restic setup.

---

## Scenario 4: Credential compromise → rotate + revoke

**Symptom:** Unauthorized API usage, leaked token, or 401/403 errors in logs.

### Recovery sequence

```bash
# 1. Inspect current credential source and expiry
aletheia credential status

# 2. If using OAuth and the grant is still valid, force a refresh
aletheia credential refresh

# 3. If the credential is compromised or refresh fails, revoke/rotate at the provider
#    - Anthropic console → revoke the old key
#    - Generate a new API key or re-authorize OAuth

# 4. Update the local credential
# Option A: env file (used by the systemd unit)
vim "$ALETHEIA_ROOT/config/env"   # set ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN

# Option B: config file
vim "$ALETHEIA_ROOT/config/aletheia.toml"
# [provider]
# api_key = "sk-ant-..."

# 5. Secure the file
chmod 600 "$ALETHEIA_ROOT/config/env"

# 6. Restart and verify
systemctl --user restart aletheia
aletheia health

# 7. Confirm no auth errors in recent logs
journalctl --user -u aletheia --since "1 minute ago" | grep -iE "401|403|auth|credential"
```

---

## Pre-flight checklist after recovery

Run these checks before declaring recovery complete:

| # | Check | Command |
|---|-------|---------|
| 1 | Service is active | `systemctl --user is-active aletheia` |
| 2 | Health endpoint returns `healthy` | `aletheia health` |
| 3 | Daemon / agent status looks normal | `aletheia status` |
| 4 | Health monitor script passes | `scripts/health-monitor.sh` |
| 5 | Metrics endpoint responds | `curl -sf http://localhost:18789/metrics \| head` |
| 6 | Session store opens without errors | `aletheia status` shows expected session counts |
| 7 | Knowledge store backup directory exists | `ls -ld "$ALETHEIA_ROOT/data/backups/fjall"` |
| 8 | Fjall backup directory exists and is writable | `ls -ld "$ALETHEIA_ROOT/data/backups/fjall"` |
| 9 | Create a test session (if auth is enabled) | See API smoke test in [DEPLOYMENT.md](DEPLOYMENT.md) |
| 10 | No recent errors in logs | `journalctl --user -u aletheia --since "5 minutes ago" --priority err..warning` |

---

## Testing recommendation: monthly DR drill

Restore to a **test instance** at least once a month to prove the procedure and detect bit-rot in backups.

```bash
# 1. Create a temporary instance root
TMP_INSTANCE=$(mktemp -d)

# 2. Restore the latest fjall backup into it
cp -a "$ALETHEIA_ROOT/data/backups/fjall/$(aletheia backup --list --json | jq -r '.[0].name')" \
      "$TMP_INSTANCE/knowledge.fjall"

# 3. Start aletheia against the test instance
aletheia -r "$TMP_INSTANCE" --port 28789 &
PID=$!
sleep 3

# 4. Run health checks
curl -sf http://localhost:28789/api/health

# 5. Clean up
kill "$PID" || true
rm -rf "$TMP_INSTANCE"
```

> If you use restic for off-machine backups, also perform a quarterly restore-from-NAS drill on a spare VM or container.

---

## Reference: key scripts and files

| Artifact | Purpose |
|----------|---------|
| `scripts/deploy.sh` | Build, copy, restart, and health-check the binary |
| `scripts/deploy.sh --rollback` | Roll back to the most recent binary backup |
| `scripts/rollback.sh` | Manual rollback (lighter-weight, no build step) |
| `scripts/health-monitor.sh` | Service health, token expiry, and metrics monitor |
| `scripts/backup-cron.sh` | Legacy script (non-functional; references removed `--export-json` flag) |
| `scripts/smoke-test.sh` | Offline CLI smoke test (good after reinstall) |
| `crates/daemon/src/maintenance/fjall_backup.rs` | Fjall knowledge store file-level backup implementation |
| `instance.example/services/aletheia.service` | Systemd unit template |
