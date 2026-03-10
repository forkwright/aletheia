//! Versioned schema migration runner.

use rusqlite::Connection;
use snafu::ResultExt;
use tracing::info;

use crate::error::{self, Result};
use crate::schema::DDL;

/// A single versioned migration.
pub struct Migration {
    /// Monotonically increasing version number.
    pub version: u32,
    /// Human-readable summary of what this migration does.
    pub description: &'static str,
    /// SQL to apply the migration.
    pub up: &'static str,
    /// SQL to reverse the migration.
    pub down: &'static str,
}

/// All registered migrations, in version order.
pub static MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "base schema — sessions, messages, usage, distillations, agent_notes",
        up: DDL,
        down: "DROP TABLE IF EXISTS agent_notes;
DROP TABLE IF EXISTS distillations;
DROP TABLE IF EXISTS usage;
DROP TABLE IF EXISTS messages;
DROP TABLE IF EXISTS sessions;",
    },
    Migration {
        version: 2,
        description: "blackboard — shared agent state with TTL",
        up: "CREATE TABLE IF NOT EXISTS blackboard (
    id TEXT PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL,
    author_nous_id TEXT NOT NULL,
    ttl_seconds INTEGER DEFAULT 3600,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    expires_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_blackboard_key ON blackboard(key);
CREATE INDEX IF NOT EXISTS idx_blackboard_expires ON blackboard(expires_at);",
        down: "DROP TABLE IF EXISTS blackboard;",
    },
    Migration {
        version: 3,
        description: "sessions display_name — user-set friendly name for sessions",
        up: "ALTER TABLE sessions ADD COLUMN display_name TEXT;",
        down: "ALTER TABLE sessions DROP COLUMN display_name;",
    },
];

/// Outcome of a migration run.
#[derive(Debug)]
pub struct MigrationResult {
    /// Versions applied during this run.
    pub applied: Vec<u32>,
    /// Schema version after migration.
    pub current_version: u32,
    /// True if the database was brand new (no tables existed).
    pub was_fresh: bool,
}

/// Pending migration info for dry-run reporting.
#[derive(Debug)]
pub struct PendingMigration {
    /// Version number that would be applied.
    pub version: u32,
    /// Human-readable summary of the migration.
    pub description: &'static str,
}

/// Apply all pending migrations to the database.
///
/// Migrations are applied in version order. Each migration runs inside a
/// transaction: the up SQL executes, then the version is recorded. If any
/// migration fails, the transaction rolls back and the error is returned.
pub fn run_migrations(conn: &Connection) -> Result<MigrationResult> {
    let was_fresh = !schema_version_table_exists(conn);

    bootstrap_version_table(conn)?;

    let current = get_schema_version(conn);
    let mut applied = Vec::new();

    for migration in MIGRATIONS {
        if migration.version <= current {
            continue;
        }

        info!(
            version = migration.version,
            description = migration.description,
            "applying migration"
        );

        let tx = conn.unchecked_transaction().context(error::DatabaseSnafu)?;

        tx.execute_batch(migration.up)
            .context(error::MigrationSnafu {
                version: migration.version,
            })?;

        tx.execute(
            "INSERT INTO schema_version (version, description) VALUES (?1, ?2)",
            rusqlite::params![migration.version, migration.description],
        )
        .context(error::MigrationSnafu {
            version: migration.version,
        })?;

        tx.commit().context(error::MigrationSnafu {
            version: migration.version,
        })?;

        applied.push(migration.version);
    }

    let current_version = applied.last().copied().unwrap_or(current);

    if !applied.is_empty() {
        info!(
            from = current,
            to = current_version,
            count = applied.len(),
            "migrations applied"
        );
    }

    Ok(MigrationResult {
        applied,
        current_version,
        was_fresh,
    })
}

/// Report pending migrations without applying them.
pub fn check_migrations(conn: &Connection) -> Result<Vec<PendingMigration>> {
    bootstrap_version_table(conn)?;
    let current = get_schema_version(conn);

    Ok(MIGRATIONS
        .iter()
        .filter(|m| m.version > current)
        .map(|m| PendingMigration {
            version: m.version,
            description: m.description,
        })
        .collect())
}

/// Ensure the `schema_version` table exists with the `description` column.
fn bootstrap_version_table(conn: &Connection) -> Result<()> {
    if schema_version_table_exists(conn) {
        // Older databases may lack the description column — add it if missing.
        if !has_description_column(conn) {
            conn.execute_batch(
                "ALTER TABLE schema_version ADD COLUMN description TEXT NOT NULL DEFAULT ''",
            )
            .context(error::DatabaseSnafu)?;
        }
        return Ok(());
    }

    conn.execute_batch(
        "CREATE TABLE schema_version (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            description TEXT NOT NULL DEFAULT ''
        )",
    )
    .context(error::DatabaseSnafu)?;

    Ok(())
}

fn schema_version_table_exists(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schema_version'",
        [],
        |row| row.get::<_, bool>(0),
    )
    .unwrap_or(false)
}

fn has_description_column(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM pragma_table_info('schema_version') WHERE name = 'description'",
        [],
        |row| row.get::<_, bool>(0),
    )
    .unwrap_or(false)
}

/// Get the current schema version, or 0 if no migrations have been applied.
pub fn get_schema_version(conn: &Connection) -> u32 {
    conn.query_row(
        "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
        [],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_conn() -> Connection {
        Connection::open_in_memory().expect("in-memory SQLite connection should always open")
    }

    #[test]
    fn fresh_database_gets_all_migrations() {
        let conn = fresh_conn();
        let result =
            run_migrations(&conn).expect("migrations should apply successfully to a fresh DB");

        assert!(result.was_fresh);
        assert_eq!(result.applied, vec![1, 2, 3]);
        assert_eq!(result.current_version, 3);
    }

    #[test]
    fn already_migrated_skips_applied() {
        let conn = fresh_conn();
        run_migrations(&conn).expect("first migration run should succeed");

        let result =
            run_migrations(&conn).expect("second migration run on same DB should succeed");
        assert!(!result.was_fresh);
        assert!(result.applied.is_empty());
        assert_eq!(result.current_version, 3);
    }

    #[test]
    fn version_recorded_in_schema_version() {
        let conn = fresh_conn();
        run_migrations(&conn).expect("migrations should apply successfully");

        let (version, description): (u32, String) = conn
            .query_row(
                "SELECT version, description FROM schema_version WHERE version = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("schema_version row for version 1 should exist after migration");
        assert_eq!(version, 1);
        assert!(!description.is_empty());
    }

    #[test]
    fn dry_run_reports_pending_without_applying() {
        let conn = fresh_conn();
        // Bootstrap table but don't apply migrations
        bootstrap_version_table(&conn).expect("bootstrap_version_table should succeed");

        let pending = check_migrations(&conn)
            .expect("check_migrations should return pending list without applying");
        assert_eq!(pending.len(), 3);
        assert_eq!(pending[0].version, 1);

        // Verify nothing was applied
        let version = get_schema_version(&conn);
        assert_eq!(version, 0);
    }

    #[test]
    fn dry_run_empty_when_current() {
        let conn = fresh_conn();
        run_migrations(&conn).expect("migrations should apply successfully");

        let pending = check_migrations(&conn)
            .expect("check_migrations should succeed on a fully migrated DB");
        assert!(pending.is_empty());
    }

    #[test]
    fn migration_order_enforced() {
        // Verify migrations are in ascending version order
        for window in MIGRATIONS.windows(2) {
            assert!(
                window[0].version < window[1].version,
                "migration {} must come before {}",
                window[0].version,
                window[1].version,
            );
        }
    }

    #[test]
    fn tables_exist_after_migration() {
        let conn = fresh_conn();
        run_migrations(&conn).expect("migrations should apply successfully");

        for table in &[
            "sessions",
            "messages",
            "usage",
            "distillations",
            "agent_notes",
            "blackboard",
        ] {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .expect("sqlite_master query should succeed for table existence check");
            assert!(exists, "table {table} should exist after migration");
        }
    }

    #[test]
    fn run_migrations_fresh_db_schema_version() {
        let conn = fresh_conn();
        let result = run_migrations(&conn).expect("migrations should apply to fresh DB");
        assert_eq!(result.current_version, 3);
        let version = get_schema_version(&conn);
        assert_eq!(version, 3);
    }

    #[test]
    fn run_migrations_idempotent() {
        let conn = fresh_conn();
        let first = run_migrations(&conn).expect("first migration run should succeed");
        let second = run_migrations(&conn).expect("second migration run should succeed idempotently");
        assert_eq!(first.current_version, second.current_version);
        assert!(second.applied.is_empty());
    }

    #[test]
    fn check_migrations_reports_pending() {
        let conn = fresh_conn();
        let pending = check_migrations(&conn)
            .expect("check_migrations should return all pending on fresh DB");
        assert_eq!(pending.len(), MIGRATIONS.len());
        assert_eq!(pending[0].version, 1);
    }

    #[test]
    fn get_schema_version_fresh_db() {
        let conn = fresh_conn();
        bootstrap_version_table(&conn).expect("bootstrap_version_table should succeed on fresh DB");
        let version = get_schema_version(&conn);
        assert_eq!(version, 0);
    }

    #[test]
    fn backward_compat_existing_v1_database() {
        let conn = fresh_conn();

        // Simulate an older database: schema_version without description column
        conn.execute_batch(
            "CREATE TABLE schema_version (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            )",
        )
        .expect("creating legacy schema_version table should succeed");
        conn.execute_batch(DDL).expect("applying base DDL to simulate v1 database should succeed");
        conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])
            .expect("inserting v1 schema_version record should succeed");

        // Running migrations should detect existing v1 and apply v2+v3
        let result = run_migrations(&conn)
            .expect("migrations should apply v2 and v3 to a v1 database");
        assert!(!result.was_fresh);
        assert_eq!(result.applied, vec![2, 3]);
        assert_eq!(result.current_version, 3);

        // description column should have been added
        assert!(has_description_column(&conn));
    }
}
