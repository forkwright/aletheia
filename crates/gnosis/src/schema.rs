//! `SQLite` schema for the gnosis code-graph index.
//!
//! # Tables
//!
//! - `symbols` — one row per public-ish definition (fn, struct, enum, trait,
//!   type alias, const) with crate + module path, name, kind, file, and line.
//! - `symbol_refs` — directed edges from one symbol site to a named target
//!   (call, reexport, impl, type-use).
//! - `crate_edges` — workspace-level crate dependency edges, loaded from
//!   `cargo metadata` and used for `crate_deps` queries.
//! - `file_hashes` — SHA-256 of each indexed file for incremental rebuilds.
//!   Re-parses only files whose hash has changed since the last index run.
//!
//! # Schema version
//!
//! Stored in `PRAGMA user_version`.  Currently `1`.  Bump when the schema
//! changes in a backward-incompatible way; `CodeGraph::open` detects a
//! mismatch and triggers a full rebuild.
//!
//! # Thread safety
//!
//! `rusqlite::Connection` is `!Send + !Sync`.  `CodeGraph` wraps it in a
//! `Mutex<Connection>` so it can be shared across async tasks.  All queries
//! lock the mutex for the duration of the call.

use rusqlite::Connection;

use crate::error::{Result, SqliteSnafu};
use snafu::ResultExt;

/// Schema version embedded in `SQLite` `user_version`.
pub(crate) const SCHEMA_VERSION: u32 = 1;

/// Initialise all tables and indexes on a fresh or opened connection.
///
/// Idempotent: uses `CREATE TABLE IF NOT EXISTS` throughout.
#[tracing::instrument(skip(conn))]
pub(crate) fn init(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA_SQL).context(SqliteSnafu)?;
    Ok(())
}

/// Full schema DDL for the gnosis index.
const SCHEMA_SQL: &str = r"
PRAGMA journal_mode = WAL;
PRAGMA synchronous  = NORMAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS symbols (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    crate_name  TEXT NOT NULL,
    module_path TEXT NOT NULL,
    symbol_name TEXT NOT NULL,
    symbol_kind TEXT NOT NULL,
    file_path   TEXT NOT NULL,
    line_start  INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_sym_crate  ON symbols(crate_name);
CREATE INDEX IF NOT EXISTS idx_sym_name   ON symbols(symbol_name);
CREATE INDEX IF NOT EXISTS idx_sym_kind   ON symbols(symbol_kind);

CREATE TABLE IF NOT EXISTS symbol_refs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    from_symbol INTEGER REFERENCES symbols(id) ON DELETE CASCADE,
    to_crate    TEXT NOT NULL,
    to_module   TEXT NOT NULL,
    to_symbol   TEXT NOT NULL,
    ref_kind    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ref_to ON symbol_refs(to_symbol, to_crate);
CREATE INDEX IF NOT EXISTS idx_ref_from ON symbol_refs(from_symbol);

CREATE TABLE IF NOT EXISTS crate_edges (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    from_crate  TEXT NOT NULL,
    to_crate    TEXT NOT NULL,
    UNIQUE(from_crate, to_crate)
);

CREATE INDEX IF NOT EXISTS idx_edge_from ON crate_edges(from_crate);
CREATE INDEX IF NOT EXISTS idx_edge_to   ON crate_edges(to_crate);

CREATE TABLE IF NOT EXISTS file_hashes (
    file_path TEXT PRIMARY KEY,
    sha256    TEXT NOT NULL
);
";
