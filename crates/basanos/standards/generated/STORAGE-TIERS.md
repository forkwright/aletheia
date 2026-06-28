# Storage Tier Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [STORAGE-TIERS/tier-selection-documented](#storage-tierstier-selection-documented)

## `STORAGE-TIERS/tier-selection-documented` {#storage-tierstier-selection-documented}

- Severity: `warning`
- Scope: `universal`
- See also: `SUBSTRATE/registration-required`

Every new storage location (table, bucket, in-memory structure) must be documented in the storage-tiers matrix with its access pattern, tier selection, and rationale. Undocumented storage choices cannot be audited, migrated, or capacity-planned. The matrix is the single source of truth for what lives where and why.

### Examples

**Good:** Choose the storage tier documented in the storage-tiers matrix for the access pattern.

```text
// structured relational data with foreign keys -> SQLite via fjall
// large binary blobs -> object store (S3-compatible)
// ephemeral cache -> in-process DashMap
```

**Bad:** Store structured query results in a flat file without justifying the tier choice.

```text
// writing Vec<Rule> to a .csv on disk
// no storage-tiers entry; cannot be audited or migrated
```

