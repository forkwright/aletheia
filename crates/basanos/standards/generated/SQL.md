# SQL Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [SQL/autoincrement-instead-of-identity](#sqlautoincrement-instead-of-identity)
- [SQL/keyword-case](#sqlkeyword-case)
- [SQL/missing-strict](#sqlmissing-strict)
- [SQL/no-if-not-exists](#sqlno-if-not-exists)
- [SQL/select-star](#sqlselect-star)

## `SQL/autoincrement-instead-of-identity` {#sqlautoincrement-instead-of-identity}

- Severity: `warning`
- Scope: `language:sql`
- Enforcer: `SQL/autoincrement-instead-of-identity`

PostgreSQL identity columns should use the SQL-standard `GENERATED ALWAYS AS IDENTITY` form instead of AUTOINCREMENT. Identity columns expose clearer metadata and avoid accidental manual inserts.

### Examples

**Good:** Use a standards-compliant identity column in PostgreSQL DDL.

```text
id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY
```

**Bad:** Use AUTOINCREMENT for a PostgreSQL identity column.

```text
id BIGINT AUTOINCREMENT PRIMARY KEY
```

## `SQL/keyword-case` {#sqlkeyword-case}

- Severity: `warning`
- Scope: `language:sql`
- Enforcer: `SQL/keyword-case`

SQL keywords should be uppercase so query structure remains visually distinct compared with identifiers and literal data. Use uppercase forms such as SELECT, FROM, and WHERE in `.sql` files and embedded SQL string literals.

### Examples

**Good:** Write SQL keywords in uppercase.

```text
SELECT id FROM users WHERE active = TRUE;
```

**Bad:** Leave SQL keywords lowercase.

```text
select id from users where active = true;
```

## `SQL/missing-strict` {#sqlmissing-strict}

- Severity: `warning`
- Scope: `language:sql`
- Enforcer: `SQL/missing-strict`
- See also: `SQL/no-if-not-exists`

SQLite `CREATE TABLE` statements should use `STRICT` so inserted data is checked against declared column types. Without strict tables, SQLite may accept mismatched data that corrupt domain assumptions silently.

### Examples

**Good:** Create a SQLite table with strict type checking enabled.

```text
CREATE TABLE users (id INTEGER PRIMARY KEY) STRICT;
```

**Bad:** Create a SQLite table without strict type checking.

```text
CREATE TABLE users (id INTEGER PRIMARY KEY);
```

## `SQL/no-if-not-exists` {#sqlno-if-not-exists}

- Severity: `warning`
- Scope: `language:sql`
- Enforcer: `SQL/no-if-not-exists`

Schema migrations must be idempotent. CREATE TABLE and ALTER TABLE ADD COLUMN statements should use IF NOT EXISTS so rerunning a migration does not fail after the schema already exists.

### Examples

**Good:** Make schema creation safe to rerun.

```text
CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY);
```

**Bad:** Create a table in a migration without an idempotence guard.

```text
CREATE TABLE users (id INTEGER PRIMARY KEY);
```

## `SQL/select-star` {#sqlselect-star}

- Severity: `warning`
- Scope: `language:sql`
- Enforcer: `SQL/select-star`

`SELECT *` returns every column, which increases payload size and couples callers to future schema changes. List the specific columns the query needs so query shape remains explicit and stable.

### Examples

**Good:** Select the columns the query actually needs.

```text
SELECT id, email FROM users;
```

**Bad:** Select every column in a table.

```text
SELECT * FROM users;
```

