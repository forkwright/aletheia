# Upgrading Aletheia

## Upgrade process

1. Check current version: `aletheia health` or `aletheia --version`
2. Back up before upgrading:
   ```bash
   aletheia backup create
   ```
3. Download the tarball from [GitHub Releases](https://github.com/forkwright/aletheia/releases):
   ```bash
   # Set VERSION to the release you are installing, e.g. v0.30.0
   VERSION=vX.Y.Z
   curl -L "https://github.com/forkwright/aletheia/releases/download/${VERSION}/aletheia-linux-x86_64-${VERSION}.tar.gz" \
     -o aletheia.tar.gz
   ```
4. Verify the checksum:
   ```bash
   sha256sum -c "aletheia-linux-x86_64-${VERSION}.tar.gz.sha256"
   ```
5. Stop the service:
   ```bash
   systemctl --user stop aletheia
   ```
6. Extract and replace the binary:
   ```bash
   tar xzf aletheia.tar.gz
   cp "aletheia-${VERSION}/aletheia" ~/.local/bin/aletheia
   ```
7. Start the service:
   ```bash
   systemctl --user start aletheia
   ```
8. Verify: `aletheia health`

### Building from source

```bash
git fetch origin && git checkout vX.Y.Z
cargo build --release
# Replace binary at /usr/local/bin/aletheia
```

---

## Config compatibility

The config system uses an owned TOML loader with `serde(default)` on all structs. New config fields added in newer versions automatically get their compiled defaults; no manual migration needed for minor versions.

Both `snake_case` and `camelCase` field names work via serde's `rename_all = "camelCase"`.

Check `git log --oneline` or [GitHub releases](https://github.com/forkwright/aletheia/releases) for breaking changes per version. Pre-1.0, MINOR bumps may include breaking changes with documented migration steps.

---

## Store migration

Sessions now use a fjall-backed store. The pre-fjall SQLite session backend is
historical. If you have a legacy SQLite `sessions.db` from aletheia 0.15.x, use
the `aletheia-sessions-migrate` one-shot tool to move session history into a
fresh fjall keyspace.

The embedded Datalog engine (knowledge store) manages its own schema versioning internally.

**Always back up before upgrading.** While migrations are tested, restoring from backup is the safest recovery path if something goes wrong.

### Migrating a legacy SQLite `sessions.db` to fjall

The migrator `crates/aletheia-sessions-migrate` (binary `aletheia-sessions-migrate`)
reads a v32 SQLite sessions database read-only and writes its contents to a new
fjall directory that matches the layout used by current aletheia. It supports:

- `--dry-run` — inspect the source DB and report the migration plan without writing.
- `--verify` — after migrating, sample rows and compare SHA-256 checksums of message bodies.
- `--verify-only` — verify a previously written destination directory.
- `--print-mapping` — print the SQLite → fjall field mapping.

**Requirements and limits:**

- Source DB must have `PRAGMA user_version = 32` (the last SQLite session schema).
- Required tables must exist: `sessions`, `messages`, `usage`, `distillations`, `agent_notes`, `blackboard`.
- Columns with no direct fjall equivalent (`thinking_enabled`, `thinking_budget`, `working_state`, `distillation_priming`) are preserved under a `migration_legacy` partition instead of dropped.
- Messages whose parent session row is missing are recovered as synthesised `orphan-recovery` sessions.
- The migrator does not migrate the knowledge store; `knowledge.fjall` must be created fresh or handled separately.

**Migration workflow:**

```bash
# 1. Stop the service
systemctl --user stop aletheia

# 2. Back up the current instance directory
cp -r instance instance-backup-$(date +%Y%m%d)

# 3. Run a dry run to confirm the source is readable
aletheia-sessions-migrate \
  --source instance/data/pre-0.16-archive/sessions.db \
  --dest instance/data/sessions.db.migrated \
  --dry-run

# 4. Migrate and verify
aletheia-sessions-migrate \
  --source instance/data/pre-0.16-archive/sessions.db \
  --dest instance/data/sessions.db.migrated \
  --verify

# 5. Swap the migrated keyspace into place
mv instance/data/sessions.db instance/data/sessions.db.pre-migration
mv instance/data/sessions.db.migrated instance/data/sessions.db

# 6. Start the service and check health
systemctl --user start aletheia
aletheia health
```

If verification fails, the migrator exits non-zero and leaves the destination
untouched. Restore from the backup taken in step 2 and inspect the mismatch report.

### Upgrading from <0.16 to >=0.16 (fjall session store) without migration

If you do not need historical session data, you can start fresh instead:

```bash
# Stop the service
systemctl --user stop aletheia

# Back up and move conflicting files
mkdir -p instance/data/pre-0.16-archive
mv instance/data/sessions.db* instance/data/pre-0.16-archive/
mv instance/data/knowledge.fjall instance/data/pre-0.16-archive/
```

The new binary will create fresh fjall stores on startup.

---


---

## Rollback

### Pre-upgrade checklist

Before any upgrade:
1. `aletheia backup`: creates a timestamped whole-instance backup set
2. Save the current binary: `cp /usr/local/bin/aletheia /usr/local/bin/aletheia.prev`
3. Record current version: `aletheia health | jq .version`

### Rollback procedure

1. Stop the service:
   ```bash
   systemctl --user stop aletheia
   ```
2. Restore the previous binary:
   ```bash
   sudo cp /usr/local/bin/aletheia.prev /usr/local/bin/aletheia
   ```
3. If the new version ran and modified the database schema, restore from the
   pre-upgrade whole-instance backup set:
   ```bash
   aletheia backup list                              # find pre-upgrade backup
   LATEST=$(aletheia backup list --json | jq -r '.[0].name')
   BACKUP="instance/data/backups/instance/${LATEST}"
   aletheia backup verify "$BACKUP"
   aletheia backup restore "$BACKUP"
   ```
4. Start the service:
   ```bash
   systemctl --user start aletheia
   ```
5. Verify: `aletheia health`

### Rollback limitations

- **Legacy SQLite migrations are forward-only.** If a newer pre-fjall version added tables or columns, an older binary may not understand the schema. Restore from backup in this case.
- **Knowledge engine schema changes** are also forward-only.
- **Config additions** in newer versions are silently ignored by older binaries (they use `serde(default)`), so config files are generally backwards-compatible.
