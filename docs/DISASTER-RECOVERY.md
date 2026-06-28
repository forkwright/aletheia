# Disaster Recovery Runbook

Operational recovery procedures for Aletheia after service failure, data corruption, machine loss, or credential compromise.

For day-to-day operations, see [RUNBOOK.md](RUNBOOK.md). Deployment details are in [DEPLOYMENT.md](DEPLOYMENT.md). The storage backend reference is [DATA.md](DATA.md).

---

## Recovery targets

| Target | Value | Basis |
|--------|-------|-------|
| **RTO** | 30–60 minutes | Local instance backup restore is copy-bound; reinstall + full NAS restore dominates the window |
| **RPO** | 24 hours | Default whole-instance backup interval (`backupIntervalHours = 24`, see `BackupSettings`) |

> If you need tighter RPO, reduce `backupIntervalHours` in `[maintenance.backup]` or run `aletheia backup` more frequently.

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

## Scenario 2: Store corruption → restore from whole-instance backup

**Symptom:** `aletheia health` reports session-store or knowledge-store failures; the store fails to open or returns read errors.

### Recovery sequence

```bash
# 1. Stop the service
systemctl --user stop aletheia

# 2. List available whole-instance backups
aletheia backup --list

# 3. Identify the most recent good backup
LATEST=$(aletheia backup --list --json | jq -r '.[0].name')
echo "Restoring from: $LATEST"
ROOT="${ALETHEIA_ROOT:-$HOME/aletheia/instance}"
BACKUP="$ROOT/data/backups/instance/$LATEST"

# 4. Verify the backup set before replacing live data
aletheia backup verify "$BACKUP"

# 5. Restore the full manifest by staging, verifying, swapping, and rolling back on failure
aletheia backup restore "$BACKUP"

# 6. Start and verify
systemctl --user start aletheia
aletheia health
```

Use `--force-live` only when the instance cannot be stopped and you accept
unsafe concurrent writes during restore. For intentional partial recovery, pass
manifest entry selectors such as `--include sessions.db` or
`--exclude logs/prompt-audit`.

### Session-store corruption

`sessions.db` is included in the whole-instance backup set under
`stores/sessions.db`. Restore it from the same backup set as `knowledge.fjall`;
do not copy a knowledge backup over `sessions.db`.

```bash
systemctl --user stop aletheia
ROOT="${ALETHEIA_ROOT:-$HOME/aletheia/instance}"
LATEST=$(aletheia backup --list --json | jq -r '.[0].name')
BACKUP="$ROOT/data/backups/instance/$LATEST"
aletheia backup verify "$BACKUP"
aletheia backup restore "$BACKUP" --include sessions.db
systemctl --user start aletheia
aletheia health
```

> Backup sets remain local under `instance/data/backups/instance/`. Use your
> own restic/ZFS/NAS process if you need off-machine copies.

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
# Edit ExecStart, EnvironmentFile, and ReadWritePaths if your layout differs.
systemd-analyze verify "$HOME/.config/systemd/user/aletheia.service"

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
| 7 | Instance backup directory exists | `ls -ld "$ALETHEIA_ROOT/data/backups/instance"` |
| 8 | Latest backup verifies | `aletheia backup verify "$ALETHEIA_ROOT/data/backups/instance/$(aletheia backup --list --json \| jq -r '.[0].name')"` |
| 9 | Create a test session (if auth is enabled) | See API smoke test in [DEPLOYMENT.md](DEPLOYMENT.md) |
| 10 | No recent errors in logs | `journalctl --user -u aletheia --since "5 minutes ago" --priority err..warning` |

---

## Testing recommendation: monthly DR drill

Restore to a **test instance** at least once a month to prove the procedure and detect bit-rot in backups.

```bash
# 1. Create a temporary instance root
TMP_INSTANCE=$(mktemp -d)

# 2. Restore the latest instance backup into it
LATEST=$(aletheia backup --list --json | jq -r '.[0].name')
BACKUP="$ALETHEIA_ROOT/data/backups/instance/$LATEST"
aletheia backup verify "$BACKUP"
aletheia -r "$TMP_INSTANCE" backup restore "$BACKUP"

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
| `scripts/backup-cron.sh` | removed legacy helper (historical note; use `aletheia backup`) |
| `scripts/smoke-test.sh` | Offline CLI smoke test (good after reinstall) |
| `crates/daemon/src/maintenance/instance_backup.rs` | Whole-instance backup set implementation |
| `crates/daemon/src/maintenance/fjall_backup.rs` | Legacy fjall store verification and snapshot helper |
| `instance.example/services/aletheia.service` | Systemd unit template |

## Emergency manual restore appendix

Use this only if the `aletheia backup restore` command is unavailable. Stop the
service first. This fallback copies every `ok` manifest entry to the same
source-relative path under the target root, but it does not provide staged
rollback.

```bash
systemctl --user stop aletheia
ROOT="${ALETHEIA_ROOT:-$HOME/aletheia/instance}"
BACKUP="$ROOT/data/backups/instance/<timestamp>"
SOURCE_ROOT=$(jq -r '.source_root' "$BACKUP/manifest.json")

jq -r '(.stores + .optional_stores)[] | select(.status == "ok") |
       [.backup_path, .source_path] | @tsv' "$BACKUP/manifest.json" |
while IFS=$'\t' read -r backup_rel source_abs; do
  rel="${source_abs#"$SOURCE_ROOT"/}"
  test "$rel" != "$source_abs" || { echo "refusing outside-root path: $source_abs" >&2; exit 1; }
  mkdir -p "$(dirname "$ROOT/$rel")"
  rm -rf "$ROOT/$rel"
  cp -a "$BACKUP/$backup_rel" "$ROOT/$rel"
done
```
