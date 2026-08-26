# Upgrading Aletheia

## Before anything else: find the binary that owns your store

**Do not run a bare `aletheia` during an upgrade.** An instance that has been upgraded
before very often has two binaries — the one its unit runs by absolute path, and an
older one still on `PATH` from an earlier install. They can be many versions apart:

```bash
$ which aletheia
/home/you/.local/bin/aletheia
$ aletheia --version
aletheia 0.14.1                 # what a bare `aletheia` resolves to

$ grep ExecStart ~/.config/systemd/user/aletheia.service
ExecStart=%h/aletheia/bin/aletheia -r %h/aletheia/instance … serve
$ ~/aletheia/bin/aletheia --version
aletheia 0.31.1                 # what actually owns the store
```

This matters more than it sounds. The store is fjall, and opening it with a binary
that does not match is the operation that can **discard data** — recovery deletes
segments absent from the levels manifest. `backup` opens the store and writes, so a
bare `aletheia backup` as step one is, on this configuration, the most destructive
command available.

Derive the path from the unit and use it for every command below:

```bash
ALETHEIA_BIN=$(systemctl --user show aletheia -p ExecStart --value \
  | sed -n 's/.*argv\[\]=\([^ ]*\).*/\1/p')
ALETHEIA_ROOT=$(systemctl --user show aletheia -p ExecStart --value \
  | sed -n 's/.*-r \([^ ]*\).*/\1/p')

"$ALETHEIA_BIN" --version           # the version that owns the store
echo "$ALETHEIA_BIN" "$ALETHEIA_ROOT"
```

If your instance is not run by a systemd unit, set both by hand from however it is
started. Do not guess, and do not fall back to `which aletheia`.

## Upgrade process

1. Confirm what owns the store — the section above. Every command below uses
   `$ALETHEIA_BIN`, never a bare `aletheia`.
2. Back up, with the binary that owns the store:
   ```bash
   "$ALETHEIA_BIN" backup
   ```
3. Download the tarball from [GitHub Releases](https://github.com/forkwright/aletheia/releases):
   ```bash
   # TAG names the GitHub release; VERSION names files inside it.
   TAG=vX.Y.Z
   VERSION="${TAG#v}"
   TARBALL="aletheia-linux-x86_64-${VERSION}.tar.gz"
   curl -fLO "https://github.com/forkwright/aletheia/releases/download/${TAG}/${TARBALL}"
   curl -fLO "https://github.com/forkwright/aletheia/releases/download/${TAG}/${TARBALL}.sha256"
   ```
4. Verify the checksum:
   ```bash
   sha256sum -c "${TARBALL}.sha256"
   ```
5. Extract, and check your config against the NEW binary before committing to it:
   ```bash
   tar xzf "$TARBALL"
   "aletheia-${VERSION}/aletheia" --version
   "aletheia-${VERSION}/aletheia" -r "$ALETHEIA_ROOT" check-config
   ```
   `check-config` validates without starting anything. Doing it here, while the old
   binary is still in place, is the difference between a config problem you fix at
   leisure and one you meet as a server that will not start after the swap. See
   [Config compatibility](#config-compatibility) for what it can report.
6. Stop the service:
   ```bash
   systemctl --user stop aletheia
   ```
7. Replace the binary **at the path the unit runs**, which is not necessarily on `PATH`:
   ```bash
   cp "$ALETHEIA_BIN" "${ALETHEIA_BIN}.prev"      # keep the rollback target
   cp "aletheia-${VERSION}/aletheia" "$ALETHEIA_BIN"
   ```
   If a stale copy is also on `PATH`, replace or delete it now — otherwise the next
   upgrade meets the same two-binary problem.
8. Start the service:
   ```bash
   systemctl --user start aletheia
   ```
9. Verify: `"$ALETHEIA_BIN" health` and `"$ALETHEIA_BIN" --version`

### Building from source

```bash
git fetch origin && git checkout vX.Y.Z
cargo build --release
cp target/release/aletheia "$ALETHEIA_BIN"    # the path the unit runs, not a guess
```

---

## Config compatibility

Run `"$NEW_BINARY" -r "$ALETHEIA_ROOT" check-config` before swapping binaries. It
validates without starting anything and reports every problem below by name.

**A config that worked can refuse to start after an upgrade.** New fields still get
their compiled defaults via `serde(default)`, and both `snake_case` and `camelCase`
work — but two things make an older config fail outright:

**Removed keys are errors, not ignored.** The config structs carry
`deny_unknown_fields`, so a key deleted in some past version is a hard refusal from the
moment that retrofit landed — including keys that were already dead long before the
binary you are running now was built. The error names the key; it does not name a
replacement, so check the release notes for the version that removed it.

**Two validations refuse the natural shape of a local single-user instance.** With
`gateway.auth.mode = "none"`, both of these are startup errors:

| Config | Why it refuses | Fix |
|---|---|---|
| `gateway.cors.allowedOrigins = []` (or `["*"]`) | With no authentication, wildcard or empty CORS lets any browser page the operator has open read responses via a cross-origin GET | List the origins explicitly, e.g. `["http://localhost:5173"]` |
| `gateway.csrf.enabled = false` | With no authentication, disabling CSRF removes the last server-side check on cross-origin *mutating* requests | `gateway.csrf.enabled = true`. `disableAcknowledged` does not waive this one — the combination is refused outright |

Both validations are correct and are not going away. They are called out here because
`auth.mode = "none"` with an empty origins list is the shape a local instance naturally
has, which makes it the likeliest config to hit this rather than an edge case.

**Default channel session keys are intentionally account-isolated after this upgrade.**
A binding that omits `sessionKey`, and any global-default route, now derives
`{channel}:{account}:{group}:{source}` instead of `{source}`. The stable account label
falls back to `default` when the provider did not attribute one, and direct messages use
`dm` for the group leg. The first post-upgrade message therefore starts a new logical
session under the isolated key; history stored under the old key is retained but is not
silently merged across accounts, channels, or groups. Explicit custom `sessionKey`
patterns are unchanged.

**Inbound operator grants move onto exact channel bindings.** The retired
`messaging.commands.operators` and `messaging.commands.defaultAllow` keys are rejected
as unknown. Grant operator commands only on a binding with an exact non-wildcard
`source`, an explicit `account`, `sourceKind = "direct"`, and
`commandTier = "operator"`. Group, wildcard, unspecified-kind, and global-default
routes remain public even if a config value is constructed outside the validated load
path.

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
- Columns with no direct fjall equivalent (`thinking_enabled`, `thinking_budget`, `working_state`, `distillation_priming`) are preserved under a `migration_legacy` partition rather than dropped.
- Messages whose parent session row is missing are recovered as synthesised `orphan-recovery` sessions.
- The migrator does not migrate the knowledge store; `knowledge.fjall` must be created fresh or handled separately.

**Migration workflow:**

```bash
# 1. Stop the service
systemctl --user stop aletheia

# 2. Back up the current instance directory
cp -r "$ALETHEIA_ROOT" "${ALETHEIA_ROOT}-backup-$(date +%Y%m%d)"

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
"$ALETHEIA_BIN" health
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

Before any upgrade, with `$ALETHEIA_BIN` resolved as in
[the first section](#before-anything-else-find-the-binary-that-owns-your-store):
1. `"$ALETHEIA_BIN" backup`: creates a timestamped whole-instance backup set
2. Save the current binary: `cp "$ALETHEIA_BIN" "${ALETHEIA_BIN}.prev"`
3. Record current version: `"$ALETHEIA_BIN" --version`

### Rollback procedure

1. Stop the service:
   ```bash
   systemctl --user stop aletheia
   ```
2. Restore the previous binary:
   ```bash
   cp "${ALETHEIA_BIN}.prev" "$ALETHEIA_BIN"
   ```
3. If the new version ran and modified the database schema, restore from the
   pre-upgrade whole-instance backup set:
   ```bash
   # WHY the restored binary and not the new one: restoring runs against the store,
   # and the whole point of a rollback is that the new version must not open it again.
   "$ALETHEIA_BIN" backup list                       # find pre-upgrade backup
   LATEST=$("$ALETHEIA_BIN" backup list --json | jq -r '.[0].name')
   BACKUP="${ALETHEIA_ROOT}/data/backups/instance/${LATEST}"
   "$ALETHEIA_BIN" backup verify "$BACKUP"
   "$ALETHEIA_BIN" backup restore "$BACKUP"
   ```
4. Start the service:
   ```bash
   systemctl --user start aletheia
   ```
5. Verify: `"$ALETHEIA_BIN" health`

### Rollback limitations

- **Legacy SQLite migrations are forward-only.** If a newer pre-fjall version added tables or columns, an older binary may not understand the schema. Restore from backup in this case.
- **Knowledge engine schema changes** are also forward-only.
- **Config files are backwards-compatible in one direction only.** A newer version's
  additions are ignored by an older binary, which uses `serde(default)`. The reverse is
  not true: `deny_unknown_fields` means a newer binary REFUSES a key an older one
  accepted. Roll the config back alongside the binary, and see
  [Config compatibility](#config-compatibility).
