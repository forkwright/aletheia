//! # gnosis — machine-derived code-graph index
//!
//! `gnosis` (γνῶσις: "knowledge") provides symbol-level cross-crate queries
//! over an aletheia workspace via a lightweight `SQLite` index built from
//! `cargo metadata` and `syn` AST walks.
//!
//! ## What it does
//!
//! Answers questions that grep + ARCHITECTURE.md cannot:
//!
//! - **`symbol_rdeps`** — "which symbols implement or re-export `Message`?"
//! - **`impl_search`** — "which types implement `Stamped`?"
//! - **`reexport_chain`** — "which crates re-export `Message` via `pub use`?"
//! - **`crate_deps`** — "what workspace crates does `nous` depend on?"
//! - **`crate_rdeps`** — "what workspace crates depend on `eidos`?"
//! - **`symbols_in`** — "list all symbols in `eidos` (optionally by kind)."
//!
//! ## What it doesn't do
//!
//! - **Replace `architecture_fact`** — that layer holds human-curated,
//!   `EpistemicTier::Verified` claims.  gnosis is machine-derived
//!   (`EpistemicTier::Inferred`).  They coexist: gnosis can verify claims, but
//!   it cannot author them.
//! - **Macro-expanded code** — `syn` operates pre-expansion.  Symbols defined
//!   only inside macros are not indexed.
//! - **Call-site resolution** — function call sites (as opposed to type
//!   references and re-exports) are not captured in v1.
//! - **Run a background daemon** — the index rebuilds on explicit
//!   [`CodeGraph::rebuild`] calls or via the `code_graph_query` MCP tool.
//!
//! ## Cache location
//!
//! Default: `~/.cache/aletheia/gnosis.sqlite`.  Override with
//! `GNOSIS_CACHE_PATH` env var.  Delete the file to force a full rebuild.
//!
//! ## Rebuild trigger
//!
//! Today: manual, via [`CodeGraph::rebuild`] or the MCP tool `op=rebuild`.
//! Future: kanon-forge-sync post-receive hook (filed as follow-up to #3357).
//!
//! ## Epistemic provenance
//!
//! Every [`query::QueryRow`] carries a `source` field (`"gnosis@<schema_version>"`)
//! so callers can detect staleness or schema mismatches.

pub mod error;
pub mod query;
pub mod schema;

mod index;

use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use rusqlite::Connection;
use snafu::ResultExt;

use crate::error::{CreateCacheDirSnafu, RemoveCacheFileSnafu, Result, SqliteSnafu};
use crate::query::QueryRow;
use crate::schema::SCHEMA_VERSION;

// ── CodeGraph ─────────────────────────────────────────────────────────────────

/// A handle to the gnosis `SQLite` index.
///
/// # Thread safety
///
/// `Connection` is `!Send`; we wrap it in `Mutex` so `CodeGraph` can be
/// stored in `Arc` and shared across async tasks.  All methods lock the
/// mutex for the duration of the call.
pub struct CodeGraph {
    /// Mutex-protected `SQLite` connection.
    conn: Mutex<Connection>,
    /// Workspace root (for rebuild).
    workspace_root: PathBuf,
    /// Gnosis schema version this instance was opened with.
    schema_version: u32,
    /// Producer string for provenance.
    producer: String,
}

impl std::fmt::Debug for CodeGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeGraph")
            .field("workspace_root", &self.workspace_root)
            .field("schema_version", &self.schema_version)
            .finish_non_exhaustive()
    }
}

impl CodeGraph {
    /// Open (or create) a gnosis index at `db_path` for `workspace_root`.
    ///
    /// If the file does not exist it is created and the schema is initialised.
    /// If the file exists but has an incompatible `user_version`, it is
    /// truncated and re-initialised (data loss is acceptable — the index is
    /// fully rebuildable).
    ///
    /// # Errors
    ///
    /// Returns [`error::GnosisError`] if the cache directory cannot be created,
    /// the database cannot be opened, or schema initialisation fails.
    #[tracing::instrument]
    pub fn open(db_path: &Path, workspace_root: &Path) -> Result<Self> {
        // Ensure the cache directory exists.
        if let Some(parent) = db_path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).with_context(|_| CreateCacheDirSnafu {
                dir: parent.to_owned(),
            })?;
        }

        let conn = Connection::open(db_path).context(SqliteSnafu)?;

        // Check schema version.
        let stored_ver: u32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .context(SqliteSnafu)?;

        if stored_ver != 0 && stored_ver != SCHEMA_VERSION {
            tracing::warn!(
                stored_ver,
                expected = SCHEMA_VERSION,
                db_path = %db_path.display(),
                "gnosis: schema version mismatch — dropping and reinitialising index"
            );
            // Drop all tables by closing and re-opening (truncate).
            drop(conn);
            std::fs::remove_file(db_path).with_context(|_| RemoveCacheFileSnafu {
                path: db_path.to_owned(),
            })?;
            let conn = Connection::open(db_path).context(SqliteSnafu)?;
            schema::init(&conn)?;
            conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))
                .context(SqliteSnafu)?;
            return Ok(Self {
                conn: Mutex::new(conn),
                workspace_root: workspace_root.to_owned(),
                schema_version: SCHEMA_VERSION,
                producer: format!("gnosis@{SCHEMA_VERSION}"),
            });
        }

        schema::init(&conn)?;
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))
            .context(SqliteSnafu)?;

        Ok(Self {
            conn: Mutex::new(conn),
            workspace_root: workspace_root.to_owned(),
            schema_version: SCHEMA_VERSION,
            producer: format!("gnosis@{SCHEMA_VERSION}"),
        })
    }

    /// Open a gnosis index at the default cache path for `workspace_root`.
    ///
    /// Default path: `~/.cache/aletheia/gnosis.sqlite` (or
    /// `GNOSIS_CACHE_PATH` env override).
    ///
    /// # Errors
    ///
    /// Same as [`CodeGraph::open`].
    pub fn open_default(workspace_root: &Path) -> Result<Self> {
        let path = Self::default_cache_path();
        Self::open(&path, workspace_root)
    }

    /// The default `SQLite` cache path.
    ///
    /// Respects `GNOSIS_CACHE_PATH` env var; falls back to
    /// `~/.cache/aletheia/gnosis.sqlite`.
    #[must_use]
    pub fn default_cache_path() -> PathBuf {
        if let Ok(p) = std::env::var("GNOSIS_CACHE_PATH")
            && !p.is_empty()
        {
            return PathBuf::from(p);
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned());
        PathBuf::from(home).join(".cache/aletheia/gnosis.sqlite")
    }

    /// Schema version this index was opened with.
    #[must_use]
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Producer provenance string, e.g. `"gnosis@1"`.
    #[must_use]
    pub fn producer(&self) -> &str {
        &self.producer
    }

    // ── Rebuild ───────────────────────────────────────────────────────────────

    /// Rebuild the index from the workspace at `workspace_root`.
    ///
    /// Only files whose hash has changed since the last rebuild are re-parsed
    /// (incremental).  To force a full rebuild, delete the cache file and call
    /// [`CodeGraph::open`] again.
    ///
    /// # Errors
    ///
    /// Returns [`error::GnosisError`] if `cargo metadata` fails, a source file
    /// cannot be read, or a `SQLite` operation fails.
    #[tracing::instrument(skip(self))]
    pub fn rebuild(&self) -> Result<()> {
        let conn = self.conn.lock();
        index::rebuild(&conn, &self.workspace_root)
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// Which symbols implement or re-export `symbol_name`?
    ///
    /// Optionally filter to refs where `to_crate = target_crate`.
    ///
    /// # Errors
    ///
    /// Returns [`error::GnosisError`] on `SQLite` failure.
    #[tracing::instrument(skip(self))]
    pub fn symbol_rdeps(
        &self,
        symbol_name: &str,
        target_crate: Option<&str>,
    ) -> Result<Vec<QueryRow>> {
        let conn = self.conn.lock();
        query::symbol_rdeps(&conn, symbol_name, target_crate)
    }

    /// Which types implement `trait_name`?
    ///
    /// # Errors
    ///
    /// Returns [`error::GnosisError`] on `SQLite` failure.
    #[tracing::instrument(skip(self))]
    pub fn impl_search(&self, trait_name: &str) -> Result<Vec<QueryRow>> {
        let conn = self.conn.lock();
        query::impl_search(&conn, trait_name)
    }

    /// Which crates re-export `symbol_name` via `pub use`?
    ///
    /// # Errors
    ///
    /// Returns [`error::GnosisError`] on `SQLite` failure.
    #[tracing::instrument(skip(self))]
    pub fn reexport_chain(&self, symbol_name: &str) -> Result<Vec<QueryRow>> {
        let conn = self.conn.lock();
        query::reexport_chain(&conn, symbol_name)
    }

    /// What workspace crates does `crate_name` directly depend on?
    ///
    /// # Errors
    ///
    /// Returns [`error::GnosisError`] on `SQLite` failure.
    #[tracing::instrument(skip(self))]
    pub fn crate_deps(&self, crate_name: &str) -> Result<Vec<QueryRow>> {
        let conn = self.conn.lock();
        query::crate_deps(&conn, crate_name)
    }

    /// What workspace crates directly depend on `crate_name`?
    ///
    /// # Errors
    ///
    /// Returns [`error::GnosisError`] on `SQLite` failure.
    #[tracing::instrument(skip(self))]
    pub fn crate_rdeps(&self, crate_name: &str) -> Result<Vec<QueryRow>> {
        let conn = self.conn.lock();
        query::crate_rdeps(&conn, crate_name)
    }

    /// List all symbols in `crate_name`, optionally filtered by `kind`.
    ///
    /// Kind values: `"fn"`, `"struct"`, `"enum"`, `"trait"`, `"type"`,
    /// `"const"`, `"impl"`, `"reexport"`.
    ///
    /// # Errors
    ///
    /// Returns [`error::GnosisError`] on `SQLite` failure.
    #[tracing::instrument(skip(self))]
    pub fn symbols_in(&self, crate_name: &str, kind: Option<&str>) -> Result<Vec<QueryRow>> {
        let conn = self.conn.lock();
        query::symbols_in(&conn, crate_name, kind)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn open_tmp_graph() -> (CodeGraph, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("gnosis.sqlite");
        let graph = CodeGraph::open(&db_path, tmp.path()).expect("open");
        (graph, tmp)
    }

    #[test]
    fn open_creates_db_and_schema() {
        let (_graph, tmp) = open_tmp_graph();
        assert!(tmp.path().join("gnosis.sqlite").exists());
    }

    #[test]
    fn schema_version_matches_constant() {
        let (graph, _tmp) = open_tmp_graph();
        assert_eq!(graph.schema_version(), SCHEMA_VERSION);
    }

    #[test]
    fn producer_string_matches_schema_version() {
        let (graph, _tmp) = open_tmp_graph();
        assert_eq!(graph.producer(), format!("gnosis@{SCHEMA_VERSION}"));
    }

    #[test]
    fn default_cache_path_respects_env() {
        // SAFETY: test-only env mutation.
        #[expect(unsafe_code, reason = "test env mutation")]
        unsafe {
            std::env::set_var("GNOSIS_CACHE_PATH", "/tmp/test-gnosis-override.sqlite");
        }
        let path = CodeGraph::default_cache_path();
        assert_eq!(path, PathBuf::from("/tmp/test-gnosis-override.sqlite"));
        #[expect(unsafe_code, reason = "test env mutation")]
        unsafe {
            std::env::remove_var("GNOSIS_CACHE_PATH");
        }
    }

    #[test]
    fn queries_return_empty_on_fresh_index() {
        let (graph, _tmp) = open_tmp_graph();
        assert!(
            graph
                .symbol_rdeps("Message", None)
                .expect("rdeps")
                .is_empty()
        );
        assert!(graph.impl_search("Stamped").expect("impl").is_empty());
        assert!(graph.reexport_chain("Foo").expect("reexport").is_empty());
        assert!(graph.crate_deps("eidos").expect("crate_deps").is_empty());
        assert!(graph.crate_rdeps("eidos").expect("crate_rdeps").is_empty());
        assert!(
            graph
                .symbols_in("eidos", None)
                .expect("symbols_in")
                .is_empty()
        );
    }
}
