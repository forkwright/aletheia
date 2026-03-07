# Upgrading Aletheia

## Upgrade Process

1. Check current version: `aletheia health` or `aletheia --version`
2. Back up before upgrading:
   ```bash
   aletheia backup
   ```
3. Download the new binary from [GitHub Releases](https://github.com/forkwright/aletheia/releases)
4. Verify checksum: `sha256sum -c aletheia-linux-amd64.sha256`
5. Stop the service:
   ```bash
   systemctl --user stop aletheia
   ```
6. Replace the binary:
   ```bash
   sudo mv aletheia-linux-amd64 /usr/local/bin/aletheia
   chmod +x /usr/local/bin/aletheia
   ```
7. Start the service:
   ```bash
   systemctl --user start aletheia
   ```
8. Verify: `aletheia health`

### Building from Source

```bash
git fetch origin && git checkout vX.Y.Z
cargo build --release
# Replace binary at /usr/local/bin/aletheia
```

---

## Config Compatibility

The config system uses figment with `serde(default)` on all structs. New config fields added in newer versions automatically get their compiled defaults — no manual migration needed for minor versions.

Both `snake_case` and `camelCase` field names work via serde's `rename_all = "camelCase"`.

Check `git log --oneline` or [GitHub releases](https://github.com/forkwright/aletheia/releases) for breaking changes per version. Pre-1.0, MINOR bumps may include breaking changes with documented migration steps.

---

## Database Migration

SQLite schema migrations run automatically on startup via `SessionStore::open()`. No manual migration steps required.

CozoDB (embedded knowledge store) manages its own schema versioning internally.

**Always back up before upgrading.** While migrations are tested, restoring from backup is the safest recovery path if something goes wrong.

---


---

## Rollback

### Pre-Upgrade Checklist

Before any upgrade:
1. `aletheia backup` — creates timestamped database backup
2. Save the current binary: `cp /usr/local/bin/aletheia /usr/local/bin/aletheia.prev`
3. Record current version: `aletheia health | jq .version`

### Rollback Procedure

1. Stop the service:
   ```bash
   systemctl --user stop aletheia
   ```
2. Restore previous binary:
   ```bash
   sudo mv /usr/local/bin/aletheia.prev /usr/local/bin/aletheia
   ```
3. If the new version ran and modified the database schema, restore from backup:
   ```bash
   aletheia backup --list          # find pre-upgrade backup
   cp instance/data/backups/<backup-file> instance/data/sessions.db
   ```
4. Start the service:
   ```bash
   systemctl --user start aletheia
   ```
5. Verify: `aletheia health`

### Rollback Limitations

- **SQLite migrations are forward-only.** If a newer version added tables or columns, the older binary may not understand the new schema. Restore from backup in this case.
- **CozoDB schema changes** are also forward-only.
- **Config additions** in newer versions are silently ignored by older binaries (they use `serde(default)`), so config files are generally backwards-compatible.
