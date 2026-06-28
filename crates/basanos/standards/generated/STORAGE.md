# Storage Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [FJALL/nested-keyspace](#fjallnested-keyspace)
- [FJALL/prefix-collect-truncate](#fjallprefix-collect-truncate)
- [STORAGE/no-migration-checksum](#storageno-migration-checksum)
- [STORAGE/no-query-timeout](#storageno-query-timeout)
- [STORAGE/sql-string-concat](#storagesql-string-concat)

## `FJALL/nested-keyspace` {#fjallnested-keyspace}

- Severity: `error`
- Scope: `language:rust`
- Enforcer: `FJALL/nested-keyspace`

`fjall::Config::new(...)` must not open a path inside an existing keyspace's `partitions/` subtree. Use `keyspace.open_partition(name, options)` so partition creation stays inside the owning keyspace instead of corrupting its manifest on later opens.

### Examples

**Good:** Open a partition through the existing keyspace handle.

```text
let partition = keyspace.open_partition(name, options)?;
```

**Bad:** Open a new keyspace inside another keyspace's partitions directory.

```text
let nested = fjall::Config::new(path.join("partitions").join(name)).open()?;
```

## `FJALL/prefix-collect-truncate` {#fjallprefix-collect-truncate}

- Severity: `error`
- Scope: `language:rust`
- Enforcer: `FJALL/prefix-collect-truncate`
- See also: `FJALL/nested-keyspace`

fjall prefix iterators must be bounded before collection. Collecting the full prefix range and then truncating holds the single-writer keyspace lock for O(partition_size); push `.rev().take(N)` directly into the iterator instead.

### Examples

**Good:** Push the bound into the prefix iterator before collecting.

```text
let entries = partition.prefix(p).rev().take(limit).collect::<Vec<_>>();
```

**Bad:** Materialize every prefix entry and truncate the tail later.

```text
let mut entries = partition.prefix(p).iter().collect::<Vec<_>>();
entries.truncate(limit);
```

## `STORAGE/no-migration-checksum` {#storageno-migration-checksum}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `STORAGE/no-migration-checksum`
- See also: `STORAGE/no-query-timeout`

Database migrations should verify checksums before applying or accepting migration history. Without checksum verification, edited migration files can silently drift from the schema state they originally created.

### Examples

**Good:** Verify migration content against a recorded checksum.

```text
verify_migration_checksum(&migration, recorded_checksum)?;
```

**Bad:** Apply migration SQL without proving its contents still match history.

```text
conn.execute_batch(include_str!("001_init.sql"))?;
```

## `STORAGE/no-query-timeout` {#storageno-query-timeout}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `STORAGE/no-query-timeout`
- See also: `SQL/missing-strict`

SQLite connections should configure a busy timeout after opening. Without a timeout, concurrent access can fail immediately with SQLITE_BUSY instead of waiting for a short, bounded interval.

### Examples

**Good:** Configure a busy timeout after opening the SQLite connection.

```text
conn.busy_timeout(Duration::from_secs(5))?;
```

**Bad:** Open a SQLite connection without configuring busy handling.

```text
let conn = Connection::open(path)?;
```

## `STORAGE/sql-string-concat` {#storagesql-string-concat}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `STORAGE/sql-string-concat`

SQL queries must not be assembled with Rust string formatting or concatenation. Use parameterized query APIs so user-controlled values stay data instead of becoming executable SQL.

### Examples

**Good:** Bind values through the query API.

```text
sqlx::query("SELECT * FROM users WHERE id = ?").bind(user_id)
```

**Bad:** Format values directly into SQL text.

```text
format!("SELECT * FROM users WHERE id = {}", user_id)
```

