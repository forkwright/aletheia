//! # gnosis — machine-derived code-graph index
//!
//! `gnosis` (γνῶσις: "knowledge") provides symbol-level cross-crate queries
//! over an aletheia workspace via a lightweight `fjall` index built from
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
//! Default: `~/.cache/aletheia/gnosis.fjall`.  Override with
//! `GNOSIS_CACHE_PATH` env var.  Delete the directory to force a full rebuild.
//!
//! ## Rebuild trigger
//!
//! Manual, via [`CodeGraph::rebuild`] or the MCP tool `op=rebuild`.
//!
//! ## Epistemic provenance
//!
//! Every [`query::QueryRow`] carries a `source` field (`"gnosis@<schema_version>"`)
//! so callers can detect staleness or schema mismatches.

#![deny(missing_docs)]

pub mod error;
pub mod query;
pub mod schema;

mod index;

use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use snafu::ResultExt;

use crate::error::{CreateCacheDirSnafu, Result};
use crate::query::QueryRow;
use crate::schema::Store;

// ── CodeGraph ─────────────────────────────────────────────────────────────────

/// A handle to the gnosis fjall index.
///
/// # Thread safety
///
/// The fjall keyspace handles are thread-safe. We keep them behind a mutex so
/// query and rebuild operations see a consistent sequence of mutations.
pub struct CodeGraph {
    /// Mutex-protected fjall store.
    store: Mutex<Store>,
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
    /// If the directory does not exist it is created and the schema is
    /// initialised. If the directory exists but carries an incompatible schema
    /// version, the index keyspaces are cleared and re-initialised (data loss
    /// is acceptable — the index is fully rebuildable).
    ///
    /// # Errors
    ///
    /// Returns [`error::GnosisError`] if the cache directory cannot be created,
    /// the fjall database cannot be opened, or schema initialisation fails.
    #[tracing::instrument]
    pub fn open(db_path: &Path, workspace_root: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).with_context(|_| CreateCacheDirSnafu {
                dir: parent.to_owned(),
            })?;
        }

        let store = Store::open(db_path)?;
        let schema_version = store.schema_version()?;

        Ok(Self {
            store: Mutex::new(store),
            workspace_root: workspace_root.to_owned(),
            schema_version,
            producer: format!("gnosis@{schema_version}"),
        })
    }

    /// Open a gnosis index at the default cache path for `workspace_root`.
    ///
    /// Default path: `~/.cache/aletheia/gnosis.fjall` (or
    /// `GNOSIS_CACHE_PATH` env override).
    ///
    /// # Errors
    ///
    /// Same as [`CodeGraph::open`].
    pub fn open_default(workspace_root: &Path) -> Result<Self> {
        let path = Self::default_cache_path();
        Self::open(&path, workspace_root)
    }

    /// The default fjall cache path.
    ///
    /// Respects `GNOSIS_CACHE_PATH` env var; falls back to
    /// `~/.cache/aletheia/gnosis.fjall`.
    #[must_use]
    pub fn default_cache_path() -> PathBuf {
        if let Ok(p) = std::env::var("GNOSIS_CACHE_PATH")
            && !p.is_empty()
        {
            return PathBuf::from(p);
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned());
        PathBuf::from(home).join(".cache/aletheia/gnosis.fjall")
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
    /// cannot be read, or a fjall operation fails.
    #[tracing::instrument(skip(self))]
    pub fn rebuild(&self) -> Result<()> {
        let store = self.store.lock();
        index::rebuild(&store, &self.workspace_root)
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// Which symbols implement or re-export `symbol_name`?
    ///
    /// Optionally filter to refs where `to_crate = target_crate`.
    ///
    /// # Errors
    ///
    /// Returns [`error::GnosisError`] on fjall failure.
    #[tracing::instrument(skip(self))]
    pub fn symbol_rdeps(
        &self,
        symbol_name: &str,
        target_crate: Option<&str>,
    ) -> Result<Vec<QueryRow>> {
        let store = self.store.lock();
        query::symbol_rdeps(&store, symbol_name, target_crate)
    }

    /// Which types implement `trait_name`?
    ///
    /// # Errors
    ///
    /// Returns [`error::GnosisError`] on fjall failure.
    #[tracing::instrument(skip(self))]
    pub fn impl_search(&self, trait_name: &str) -> Result<Vec<QueryRow>> {
        let store = self.store.lock();
        query::impl_search(&store, trait_name)
    }

    /// Which crates re-export `symbol_name` via `pub use`?
    ///
    /// # Errors
    ///
    /// Returns [`error::GnosisError`] on fjall failure.
    #[tracing::instrument(skip(self))]
    pub fn reexport_chain(&self, symbol_name: &str) -> Result<Vec<QueryRow>> {
        let store = self.store.lock();
        query::reexport_chain(&store, symbol_name)
    }

    /// What workspace crates does `crate_name` directly depend on?
    ///
    /// # Errors
    ///
    /// Returns [`error::GnosisError`] on fjall failure.
    #[tracing::instrument(skip(self))]
    pub fn crate_deps(&self, crate_name: &str) -> Result<Vec<QueryRow>> {
        let store = self.store.lock();
        query::crate_deps(&store, crate_name)
    }

    /// What workspace crates directly depend on `crate_name`?
    ///
    /// # Errors
    ///
    /// Returns [`error::GnosisError`] on fjall failure.
    #[tracing::instrument(skip(self))]
    pub fn crate_rdeps(&self, crate_name: &str) -> Result<Vec<QueryRow>> {
        let store = self.store.lock();
        query::crate_rdeps(&store, crate_name)
    }

    /// List all symbols in `crate_name`, optionally filtered by `kind`.
    ///
    /// Kind values: `"fn"`, `"struct"`, `"enum"`, `"trait"`, `"type"`,
    /// `"const"`, `"impl"`, `"reexport"`.
    ///
    /// # Errors
    ///
    /// Returns [`error::GnosisError`] on fjall failure.
    #[tracing::instrument(skip(self))]
    pub fn symbols_in(&self, crate_name: &str, kind: Option<&str>) -> Result<Vec<QueryRow>> {
        let store = self.store.lock();
        query::symbols_in(&store, crate_name, kind)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn open_tmp_graph() -> (CodeGraph, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("gnosis.fjall");
        let graph = CodeGraph::open(&db_path, tmp.path()).expect("open");
        (graph, tmp)
    }

    #[test]
    fn open_creates_db_and_schema() {
        let (_graph, tmp) = open_tmp_graph();
        assert!(tmp.path().join("gnosis.fjall").exists());
    }

    #[test]
    fn schema_version_matches_constant() {
        let (graph, _tmp) = open_tmp_graph();
        assert_eq!(graph.schema_version(), crate::schema::SCHEMA_VERSION);
    }

    #[test]
    fn producer_string_matches_schema_version() {
        let (graph, _tmp) = open_tmp_graph();
        assert_eq!(
            graph.producer(),
            format!("gnosis@{}", crate::schema::SCHEMA_VERSION)
        );
    }

    #[test]
    fn default_cache_path_respects_env() {
        // SAFETY: test-only env mutation.
        #[expect(unsafe_code, reason = "test env mutation")]
        unsafe {
            std::env::set_var("GNOSIS_CACHE_PATH", "/tmp/test-gnosis-override.fjall");
        }
        let path = CodeGraph::default_cache_path();
        assert_eq!(path, PathBuf::from("/tmp/test-gnosis-override.fjall"));
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
