# Restic Backup Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [RESTIC/verify-after-backup](#resticverify-after-backup)

## `RESTIC/verify-after-backup` {#resticverify-after-backup}

- Severity: `warning`
- Scope: `project:ops`

Every automated restic backup job must run restic check after the backup completes. A backup that has never been verified has unknown integrity: corruption in the object store or an interrupted write can silently invalidate the snapshot, which is only discovered at restore time.

### Examples

**Good:** Run restic check after every backup job to confirm snapshot integrity.

```text
restic backup /data && restic check --read-data-subset=1/10
```

**Bad:** Take a backup without checking whether the repository is still consistent.

```text
restic backup /data
# no check step; corruption is undetected until restore
```

