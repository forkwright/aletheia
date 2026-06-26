//! Structured facts about the codebase that research agents can query.
//!
//! An [`ArchitectureFact`] is a short, cited, versioned claim about an
//! architectural seam: spawn model, storage invariants, hook taxonomy,
//! lifecycle boundaries, etc.  Facts are written once (by whoever changes
//! the shape) and reused by every downstream research agent at O(query)
//! cost instead of O(full-codebase-read).
//!
//! # Storage
//!
//! [`FactStore`] uses flat JSON files under a configurable directory
//! (default: `~/aletheia/instance/facts/`).  Each fact is serialised to
//! `<dir>/<id>.json` where `<id>` is the dot-separated fact identifier
//! with path separators replaced by `-` to produce a safe filename.  No
//! external database is required — the directory is created on first write.
//!
//! # Design constraints
//!
//! - `serde_json` is already in the `eidos` dependency tree; no new deps.
//! - Facts carry no credentials: `ArchitectureFact` fields are public
//!   knowledge about the codebase, never secrets.
//! - The `updated_by` field records the PR number or session key that last
//!   touched the fact, providing a lightweight audit trail.
//!
//! # Search
//!
//! [`FactStore::search`] performs a case-insensitive substring scan across
//! all loaded facts' `id`, `scope`, and `claim` fields.  Full-text
//! substring scanning is sufficient for the tracked v1 size limit (<1 000
//! facts).

use std::io;
use std::path::PathBuf;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use tokio::sync::Mutex;

/// Errors from [`FactStore`] operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[expect(
    missing_docs,
    reason = "snafu error variant fields are self-documenting via display format"
)]
#[non_exhaustive]
pub enum FactError {
    /// The facts directory could not be created.
    #[snafu(display("failed to create facts directory {}: {source}", dir.display()))]
    CreateDir { dir: PathBuf, source: io::Error },

    /// A fact file could not be read.
    #[snafu(display("failed to read fact file {}: {source}", path.display()))]
    ReadFile { path: PathBuf, source: io::Error },

    /// A fact file could not be written.
    #[snafu(display("failed to write fact file {}: {source}", path.display()))]
    WriteFile { path: PathBuf, source: io::Error },

    /// The facts directory could not be listed.
    #[snafu(display("failed to read facts directory {}: {source}", dir.display()))]
    ReadDir { dir: PathBuf, source: io::Error },

    /// A directory entry could not be inspected.
    #[snafu(display("failed to inspect directory entry in {}: {source}", dir.display()))]
    DirEntry { dir: PathBuf, source: io::Error },

    /// JSON deserialisation of a fact file failed.
    #[snafu(display("failed to deserialise fact from {}: {source}", path.display()))]
    Deserialise {
        path: PathBuf,
        source: serde_json::Error,
    },

    /// JSON serialisation of a fact failed.
    #[snafu(display("failed to serialise fact {id}: {source}"))]
    Serialise {
        id: String,
        source: serde_json::Error,
    },
}

/// Architectural scope that a fact applies to.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum FactScope {
    /// Crate-level fact (ownership, public API surface, dependency direction).
    Crate,
    /// Module-level fact (internal structure, visibility rules).
    Module,
    /// Concept-level fact (a design pattern, invariant, or protocol).
    Concept,
    /// Boundary fact (how two systems interact at their interface).
    Boundary,
}

impl std::fmt::Display for FactScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Crate => f.write_str("crate"),
            Self::Module => f.write_str("module"),
            Self::Concept => f.write_str("concept"),
            Self::Boundary => f.write_str("boundary"),
        }
    }
}

/// A single structured fact about the codebase, queryable by research agents.
///
/// # Naming convention for `id`
///
/// Use dot-separated hierarchical paths: `<project>.<subsystem>.<aspect>`.
/// Examples:
/// - `aletheia.spawn.model`
/// - `aletheia.providers.llm.supported`
/// - `aletheia.graphe.single-writer-invariant`
///
/// # `updated_by`
///
/// Record the PR number (`PR-3789`) or session key (`session_<id>`) that
/// last wrote this fact.  This is provenance, not a credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ArchitectureFact {
    /// Stable dot-separated identifier, e.g. `aletheia.spawn.model`.
    pub id: String, // kanon:ignore RUST/primitive-for-domain-id — dot-separated hierarchical path, not a typed domain ID
    /// Architectural scope of this fact.
    pub scope: FactScope,
    /// The fact itself, written as a single declarative sentence (markdown OK).
    pub claim: String,
    /// File paths or URLs that support the claim.
    pub evidence: Vec<String>,
    /// `session_key` of a supporting `mneme` annotation, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mneme_session: Option<String>,
    /// RFC 3339 timestamp of last update.
    pub updated_at: String,
    /// PR number or session key that last touched this fact.
    pub updated_by: String,
}

impl ArchitectureFact {
    /// Construct a new fact with the current UTC timestamp.
    ///
    /// `updated_by` should be a PR number (`PR-3789`) or session key.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        scope: FactScope,
        claim: impl Into<String>,
        evidence: Vec<String>,
        updated_by: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            scope,
            claim: claim.into(),
            evidence,
            mneme_session: None,
            updated_at: Timestamp::now().strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
            updated_by: updated_by.into(),
        }
    }
}

/// Flat-JSON-file backed store for [`ArchitectureFact`]s.
///
/// One file per fact: `<dir>/<safe_id>.json` where `<safe_id>` is the fact's
/// `id` with `/` and `\` replaced by `-`. Dots are preserved.
///
/// The store is created lazily: the directory is created on first [`put`].
///
/// [`put`]: FactStore::put
pub struct FactStore {
    dir: PathBuf,
    /// In-memory cache of loaded facts and precomputed lowercase search keys.
    ///
    /// WHY: `list` and `search` are called repeatedly in a single agent turn;
    /// populating this once per `FactStore` lifetime avoids cold file reads on
    /// every query.  `put` keeps the cache up to date so subsequent reads do not
    /// need to reload from disk.
    cache: Mutex<Option<Vec<IndexedFact>>>,
}

/// In-memory indexed representation of a fact.
///
/// WHY: search filters by case-insensitive substring; storing lowercase copies
/// of `id`, `scope`, and `claim` once at load/put time removes per-candidate
/// allocations on the hot path.
struct IndexedFact {
    fact: ArchitectureFact,
    id_lower: String,
    claim_lower: String,
    scope_lower: String,
}

impl IndexedFact {
    /// Build an indexed fact, precomputing lowercase search keys.
    fn new(fact: ArchitectureFact) -> Self {
        Self {
            id_lower: fact.id.to_lowercase(),
            scope_lower: fact.scope.to_string().to_lowercase(),
            claim_lower: fact.claim.to_lowercase(),
            fact,
        }
    }
}

impl FactStore {
    /// Create a store rooted at `dir`.
    ///
    /// The directory is created on the first [`put`] call; it does not need
    /// to exist when the store is constructed.
    ///
    /// [`put`]: FactStore::put
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            cache: Mutex::new(None),
        }
    }

    /// Default store path: `~/aletheia/instance/facts/`.
    #[must_use]
    pub fn default_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned());
        PathBuf::from(home).join("aletheia/instance/facts")
    }

    /// Translate a fact `id` to a safe filename (no path separators).
    fn id_to_filename(id: &str) -> String {
        let mut name: String = id
            .chars()
            .map(|c| match c {
                '/' | '\\' => '-',
                _ => c,
            })
            .collect();
        name.push_str(".json");
        name
    }

    fn fact_path(&self, id: &str) -> PathBuf {
        self.dir.join(Self::id_to_filename(id))
    }

    /// Retrieve a fact by exact `id`.  Returns `None` if no fact with that id
    /// exists.
    ///
    /// # Errors
    ///
    /// Returns [`FactError`] if the file exists but cannot be read or parsed.
    #[tracing::instrument(skip(self))]
    pub async fn get(&self, id: &str) -> Result<Option<ArchitectureFact>, FactError> {
        let path = self.fact_path(id);
        if !tokio::fs::try_exists(&path)
            .await
            .with_context(|_| ReadFileSnafu { path: path.clone() })?
        {
            return Ok(None);
        }
        let bytes = tokio::fs::read(&path)
            .await
            .with_context(|_| ReadFileSnafu { path: path.clone() })?;
        let fact = serde_json::from_slice(&bytes)
            .with_context(|_| DeserialiseSnafu { path: path.clone() })?;
        Ok(Some(fact))
    }

    /// Write a fact to the store.  Creates the directory if it does not exist.
    /// Overwrites any existing fact with the same `id`.
    ///
    /// # Errors
    ///
    /// Returns [`FactError`] if the directory cannot be created, serialisation
    /// fails, or the file cannot be written.
    #[tracing::instrument(skip(self, fact), fields(id = %fact.id))]
    pub async fn put(&self, fact: ArchitectureFact) -> Result<(), FactError> {
        tokio::fs::create_dir_all(&self.dir)
            .await
            .with_context(|_| CreateDirSnafu {
                dir: self.dir.clone(),
            })?;
        let path = self.fact_path(&fact.id);
        let json = serde_json::to_vec_pretty(&fact).with_context(|_| SerialiseSnafu {
            id: fact.id.clone(),
        })?;
        tokio::fs::write(&path, &json)
            .await
            .with_context(|_| WriteFileSnafu { path })?;
        let mut guard = self.cache.lock().await;
        if let Some(idx) = guard.as_mut() {
            let id = fact.id.clone();
            if let Some(pos) = idx.iter().position(|i| i.fact.id == id) {
                // INVARIANT: pos comes from position() on idx with no intervening mutation
                if let Some(entry) = idx.get_mut(pos) {
                    *entry = IndexedFact::new(fact);
                }
            } else {
                idx.push(IndexedFact::new(fact));
            }
        }
        Ok(())
    }

    /// List all facts, optionally filtered to a specific scope.
    ///
    /// # Errors
    ///
    /// Returns [`FactError`] if the directory cannot be read or a fact file
    /// cannot be parsed.
    #[tracing::instrument(skip(self))]
    pub async fn list(&self, scope: Option<FactScope>) -> Result<Vec<ArchitectureFact>, FactError> {
        self.ensure_loaded().await?;
        let guard = self.cache.lock().await;
        let Some(idx) = guard.as_ref() else {
            return Ok(vec![]);
        };
        Ok(idx
            .iter()
            .filter(|i| scope.is_none_or(|s| i.fact.scope == s))
            .map(|i| i.fact.clone())
            .collect())
    }

    /// Search facts by case-insensitive substring match across `id`, `scope`,
    /// and `claim`.
    ///
    /// # Errors
    ///
    /// Returns [`FactError`] if the directory cannot be read or a fact file
    /// cannot be parsed.
    #[tracing::instrument(skip(self))]
    pub async fn search(&self, query: &str) -> Result<Vec<ArchitectureFact>, FactError> {
        self.ensure_loaded().await?;
        let query_lower = query.to_lowercase();
        let guard = self.cache.lock().await;
        let Some(idx) = guard.as_ref() else {
            return Ok(vec![]);
        };
        Ok(idx
            .iter()
            .filter(|i| {
                i.id_lower.contains(&query_lower)
                    || i.scope_lower.contains(&query_lower)
                    || i.claim_lower.contains(&query_lower)
            })
            .map(|i| i.fact.clone())
            .collect())
    }

    /// Ensure the in-memory cache has been populated.
    ///
    /// WHY: This is the single place `load_all` is invoked.  After the first
    /// successful load the result is cached, so repeated `list`/`search` calls
    /// in one agent turn do not re-read every fact file from disk.
    async fn ensure_loaded(&self) -> Result<(), FactError> {
        {
            let guard = self.cache.lock().await;
            if guard.is_some() {
                return Ok(());
            }
        }

        let dir_exists = tokio::fs::try_exists(&self.dir)
            .await
            .with_context(|_| ReadDirSnafu {
                dir: self.dir.clone(),
            })?;
        let indexed = if dir_exists {
            self.load_all()
                .await?
                .into_iter()
                .map(IndexedFact::new)
                .collect()
        } else {
            Vec::new()
        };

        let mut guard = self.cache.lock().await;
        if guard.is_none() {
            *guard = Some(indexed);
        }
        Ok(())
    }

    /// Load all `.json` files in the store directory.
    async fn load_all(&self) -> Result<Vec<ArchitectureFact>, FactError> {
        let mut entries = tokio::fs::read_dir(&self.dir)
            .await
            .with_context(|_| ReadDirSnafu {
                dir: self.dir.clone(),
            })?;
        let mut facts = Vec::new();
        while let Some(entry) = entries.next_entry().await.with_context(|_| DirEntrySnafu {
            dir: self.dir.clone(),
        })? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let bytes = tokio::fs::read(&path)
                .await
                .with_context(|_| ReadFileSnafu { path: path.clone() })?;
            let fact: ArchitectureFact = serde_json::from_slice(&bytes)
                .with_context(|_| DeserialiseSnafu { path: path.clone() })?;
            facts.push(fact);
        }
        Ok(facts)
    }
}

/// Build the five seed facts that describe aletheia's own architecture.
///
/// These are written by `PR-3789` and provide the initial populated state
/// of the fact store, so that research agents have a working example of
/// what the layer contains.
#[must_use]
pub fn seed_facts() -> Vec<ArchitectureFact> {
    vec![
        ArchitectureFact {
            id: "aletheia.spawn.model".to_owned(),
            scope: FactScope::Concept,
            claim: "Aletheia spawns agents as in-process Tokio tasks, not as subprocesses. \
                    `nous::spawn_svc` drives the lifecycle; no `std::process::Command` is \
                    involved."
                .to_owned(),
            evidence: vec!["crates/nous/src/spawn_svc.rs:56-99".to_owned()],
            mneme_session: None,
            updated_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_by: "PR-3789".to_owned(),
        },
        ArchitectureFact {
            id: "aletheia.graphe.single-writer-invariant".to_owned(),
            scope: FactScope::Concept,
            claim: "The `graphe` session graph is single-writer: only the owning actor may \
                    mutate its state. Reads are behind a `RwLock`; writes require an exclusive \
                    guard obtained from the session actor."
                .to_owned(),
            evidence: vec!["crates/graphe/src/session_actor.rs".to_owned()],
            mneme_session: None,
            updated_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_by: "PR-3789".to_owned(),
        },
        ArchitectureFact {
            id: "aletheia.providers.llm.routing".to_owned(),
            scope: FactScope::Boundary,
            claim: "LLM provider routing is configured in the fleet dispatch config \
                    (`<fleet-dispatch-config>`). The `local` provider routes to \
                    logismos at its configured port; `anthropic` routes to the Anthropic API. \
                    Provider selection is per-nous-config, not global."
                .to_owned(),
            evidence: vec![
                "<fleet-dispatch-config>".to_owned(),
                "crates/hermeneus/src/provider.rs".to_owned(),
            ],
            mneme_session: None,
            updated_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_by: "PR-3789".to_owned(),
        },
        ArchitectureFact {
            id: "aletheia.eidos.dependency-direction".to_owned(),
            scope: FactScope::Crate,
            claim: "`eidos` is the foundational types crate: it has zero internal aletheia \
                    dependencies. All other crates may depend on `eidos`; `eidos` must not \
                    depend on any other fleet crate."
                .to_owned(),
            evidence: vec!["crates/eidos/Cargo.toml".to_owned()],
            mneme_session: None,
            updated_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_by: "PR-3789".to_owned(),
        },
        ArchitectureFact {
            id: "aletheia.organon.tool-registration".to_owned(),
            scope: FactScope::Module,
            claim: "All built-in MCP tools are registered via `builtins::register_all` in \
                    `crates/organon/src/builtins/mod.rs`. Each builtin module exposes a \
                    `pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()>` \
                    that the top-level `register_all` calls. New tools follow this pattern."
                .to_owned(),
            evidence: vec!["crates/organon/src/builtins/mod.rs".to_owned()],
            mneme_session: None,
            updated_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_by: "PR-3789".to_owned(),
        },
    ]
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    /// Build a minimal fact for tests that only care about id/scope/claim.
    fn test_fact(id: &str, scope: FactScope, claim: &str) -> ArchitectureFact {
        ArchitectureFact {
            id: id.to_owned(),
            scope,
            claim: claim.to_owned(),
            evidence: vec![],
            mneme_session: None,
            updated_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_by: "PR-1".to_owned(),
        }
    }

    #[test]
    fn serde_round_trip() {
        let fact = ArchitectureFact {
            id: "test.fact.one".to_owned(),
            scope: FactScope::Concept,
            claim: "Agents spawn in-process.".to_owned(),
            evidence: vec!["crates/nous/src/spawn_svc.rs:56".to_owned()],
            mneme_session: Some("session_abc".to_owned()),
            updated_at: "2026-04-22T12:00:00Z".to_owned(),
            updated_by: "PR-9999".to_owned(),
        };
        let json = serde_json::to_string(&fact).expect("serialise");
        let back: ArchitectureFact = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back.id, fact.id);
        assert_eq!(back.scope, fact.scope);
        assert_eq!(back.claim, fact.claim);
        assert_eq!(back.evidence, fact.evidence);
        assert_eq!(back.mneme_session, fact.mneme_session);
        assert_eq!(back.updated_by, fact.updated_by);
    }

    #[test]
    fn serde_optional_mneme_session_omitted_when_none() {
        let fact = ArchitectureFact {
            id: "test.fact.two".to_owned(),
            scope: FactScope::Crate,
            claim: "eidos has no internal deps.".to_owned(),
            evidence: vec![],
            mneme_session: None,
            updated_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_by: "PR-3789".to_owned(),
        };
        let json = serde_json::to_string(&fact).expect("serialise");
        assert!(
            !json.contains("mneme_session"),
            "mneme_session should be omitted when None"
        );
    }

    #[test]
    fn new_sets_jiff_parseable_rfc3339_timestamp() {
        let fact = ArchitectureFact::new(
            "test.fact.timestamp",
            FactScope::Concept,
            "ArchitectureFact::new timestamps are jiff parseable.",
            Vec::new(),
            "test",
        );

        let parsed = fact
            .updated_at
            .parse::<Timestamp>()
            .expect("updated_at parses as jiff timestamp");
        assert_eq!(
            parsed.strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
            fact.updated_at,
            "timestamp should round-trip through jiff with second precision"
        );
    }

    #[test]
    fn fact_scope_serde_all_variants() {
        for (scope, expected) in [
            (FactScope::Crate, "\"crate\""),
            (FactScope::Module, "\"module\""),
            (FactScope::Concept, "\"concept\""),
            (FactScope::Boundary, "\"boundary\""),
        ] {
            let json = serde_json::to_string(&scope).expect("serialise scope");
            assert_eq!(json, expected, "scope {scope:?} serialise mismatch");
            let back: FactScope = serde_json::from_str(&json).expect("deserialise scope");
            assert_eq!(back, scope, "scope {scope:?} round-trip failed");
        }
    }

    #[test]
    fn fact_scope_display() {
        assert_eq!(FactScope::Crate.to_string(), "crate");
        assert_eq!(FactScope::Module.to_string(), "module");
        assert_eq!(FactScope::Concept.to_string(), "concept");
        assert_eq!(FactScope::Boundary.to_string(), "boundary");
    }

    #[tokio::test]
    async fn put_then_get_returns_fact() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FactStore::new(dir.path());
        let fact = ArchitectureFact {
            id: "test.put.get".to_owned(),
            scope: FactScope::Boundary,
            claim: "Test claim.".to_owned(),
            evidence: vec!["src/lib.rs:1".to_owned()],
            mneme_session: None,
            updated_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_by: "PR-1".to_owned(),
        };
        store.put(fact.clone()).await.expect("put");
        let got = store.get("test.put.get").await.expect("get");
        let got = got.expect("fact should exist");
        assert_eq!(got.id, "test.put.get");
        assert_eq!(got.claim, "Test claim.");
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FactStore::new(dir.path());
        let result = store.get("nonexistent.fact").await.expect("get");
        assert!(result.is_none(), "missing fact should return None");
    }

    #[tokio::test]
    async fn list_all_facts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FactStore::new(dir.path());
        for i in 0..3u32 {
            let fact = ArchitectureFact {
                id: format!("test.list.{i}"),
                scope: if i == 0 {
                    FactScope::Crate
                } else {
                    FactScope::Concept
                },
                claim: format!("Claim {i}."),
                evidence: vec![],
                mneme_session: None,
                updated_at: "2026-04-22T00:00:00Z".to_owned(),
                updated_by: "PR-1".to_owned(),
            };
            store.put(fact).await.expect("put");
        }
        let all = store.list(None).await.expect("list all");
        assert_eq!(all.len(), 3, "expected 3 facts, got {}", all.len());
    }

    #[tokio::test]
    async fn list_filtered_by_scope() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FactStore::new(dir.path());
        store
            .put(ArchitectureFact {
                id: "test.scope.crate".to_owned(),
                scope: FactScope::Crate,
                claim: "Crate fact.".to_owned(),
                evidence: vec![],
                mneme_session: None,
                updated_at: "2026-04-22T00:00:00Z".to_owned(),
                updated_by: "PR-1".to_owned(),
            })
            .await
            .expect("put");
        store
            .put(ArchitectureFact {
                id: "test.scope.concept".to_owned(),
                scope: FactScope::Concept,
                claim: "Concept fact.".to_owned(),
                evidence: vec![],
                mneme_session: None,
                updated_at: "2026-04-22T00:00:00Z".to_owned(),
                updated_by: "PR-1".to_owned(),
            })
            .await
            .expect("put");
        let crates = store
            .list(Some(FactScope::Crate))
            .await
            .expect("list crate");
        assert_eq!(crates.len(), 1, "expected 1 crate fact");
        #[expect(clippy::indexing_slicing, reason = "test assertion: len checked above")]
        let crate_scope = crates[0].scope;
        assert_eq!(crate_scope, FactScope::Crate);
    }

    #[tokio::test]
    async fn search_by_claim_substring() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FactStore::new(dir.path());
        store
            .put(ArchitectureFact {
                id: "test.search.one".to_owned(),
                scope: FactScope::Concept,
                claim: "Agents spawn as Tokio tasks.".to_owned(),
                evidence: vec![],
                mneme_session: None,
                updated_at: "2026-04-22T00:00:00Z".to_owned(),
                updated_by: "PR-1".to_owned(),
            })
            .await
            .expect("put");
        store
            .put(ArchitectureFact {
                id: "test.search.two".to_owned(),
                scope: FactScope::Crate,
                claim: "eidos has no internal deps.".to_owned(),
                evidence: vec![],
                mneme_session: None,
                updated_at: "2026-04-22T00:00:00Z".to_owned(),
                updated_by: "PR-1".to_owned(),
            })
            .await
            .expect("put");
        let results = store.search("tokio").await.expect("search");
        assert_eq!(results.len(), 1, "expected 1 result for 'tokio'");
        #[expect(clippy::indexing_slicing, reason = "test assertion: len checked above")]
        let result_id = results[0].id.clone();
        assert_eq!(result_id, "test.search.one");
    }

    #[tokio::test]
    async fn search_empty_when_no_match() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FactStore::new(dir.path());
        store
            .put(ArchitectureFact {
                id: "test.search.nomatch".to_owned(),
                scope: FactScope::Crate,
                claim: "Something unrelated.".to_owned(),
                evidence: vec![],
                mneme_session: None,
                updated_at: "2026-04-22T00:00:00Z".to_owned(),
                updated_by: "PR-1".to_owned(),
            })
            .await
            .expect("put");
        let results = store.search("xyzzy_not_found").await.expect("search");
        assert!(
            results.is_empty(),
            "expected no results for nonexistent query"
        );
    }

    #[tokio::test]
    async fn list_returns_empty_when_dir_missing() {
        // Directory that does not exist — should return empty, not error.
        let store = FactStore::new("/tmp/aletheia-facts-does-not-exist-xyzzy-12345");
        let result = store.list(None).await.expect("list");
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn search_returns_empty_when_dir_missing() {
        // Directory that does not exist — should return empty, not error.
        let store = FactStore::new("/tmp/aletheia-facts-does-not-exist-xyzzy-search");
        let result = store.search("tokio").await.expect("search");
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn list_reuses_cache_after_source_file_removed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FactStore::new(dir.path());
        store
            .put(test_fact("test.cache.one", FactScope::Concept, "Cached."))
            .await
            .expect("put");
        let first = store.list(None).await.expect("list");
        assert_eq!(first.len(), 1, "expected fact before removal");

        // Remove the backing file. A second list that re-reads disk would return empty.
        let path = dir.path().join("test.cache.one.json");
        tokio::fs::remove_file(&path).await.expect("remove file");

        let second = store.list(None).await.expect("list cached");
        assert_eq!(second.len(), 1, "list should reuse the in-memory cache");
        #[expect(clippy::indexing_slicing, reason = "test assertion: len checked above")]
        let cached_id = second[0].id.clone();
        assert_eq!(cached_id, "test.cache.one");
    }

    #[tokio::test]
    async fn search_reuses_cache_after_source_file_removed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FactStore::new(dir.path());
        store
            .put(test_fact(
                "test.cache.search",
                FactScope::Boundary,
                "Find me.",
            ))
            .await
            .expect("put");
        let first = store.search("find").await.expect("search");
        assert_eq!(first.len(), 1);

        let path = dir.path().join("test.cache.search.json");
        tokio::fs::remove_file(&path).await.expect("remove file");

        let second = store.search("FIND").await.expect("search cached");
        assert_eq!(second.len(), 1, "search should reuse the in-memory cache");
        #[expect(clippy::indexing_slicing, reason = "test assertion: len checked above")]
        let cached_id = second[0].id.clone();
        assert_eq!(cached_id, "test.cache.search");
    }

    #[tokio::test]
    async fn put_updates_cached_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FactStore::new(dir.path());
        store
            .put(test_fact("test.cache.a", FactScope::Concept, "A"))
            .await
            .expect("put");
        let first = store.list(None).await.expect("list");
        assert_eq!(first.len(), 1, "expected one fact after initial put");

        store
            .put(test_fact("test.cache.b", FactScope::Crate, "B"))
            .await
            .expect("put second");
        let second = store.list(None).await.expect("list after second put");
        assert_eq!(
            second.len(),
            2,
            "put should keep the in-memory cache up to date"
        );
    }

    #[tokio::test]
    async fn search_matches_id_scope_and_claim_case_insensitively() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FactStore::new(dir.path());
        store
            .put(test_fact("test.cache.id", FactScope::Crate, "Claim text."))
            .await
            .expect("put");

        let by_id = store.search("TEST.CACHE.ID").await.expect("search id");
        assert_eq!(by_id.len(), 1, "search should match id case-insensitively");

        let by_scope = store.search("CRATE").await.expect("search scope");
        assert_eq!(
            by_scope.len(),
            1,
            "search should match scope case-insensitively"
        );

        let by_claim = store.search("CLAIM TEXT").await.expect("search claim");
        assert_eq!(
            by_claim.len(),
            1,
            "search should match claim case-insensitively"
        );
    }

    #[test]
    fn seed_facts_are_valid() {
        let facts = seed_facts();
        assert_eq!(facts.len(), 5, "expected 5 seed facts");
        for fact in &facts {
            assert!(!fact.id.is_empty(), "fact id must not be empty");
            assert!(!fact.claim.is_empty(), "fact claim must not be empty");
            assert_eq!(fact.updated_by, "PR-3789");
        }
    }

    #[test]
    fn provider_routing_seed_evidence_resolves() {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates dir")
            .parent()
            .expect("workspace root");
        let fact = seed_facts()
            .into_iter()
            .find(|fact| fact.id == "aletheia.providers.llm.routing")
            .expect("provider routing seed fact");

        let repo_paths: Vec<&str> = fact
            .evidence
            .iter()
            .map(String::as_str)
            .filter(|path| path.starts_with("crates/"))
            .collect();
        assert!(!repo_paths.is_empty(), "seed fact should cite repo paths");
        for path in repo_paths {
            assert!(
                workspace_root.join(path).exists(),
                "seed evidence path should resolve: {path}"
            );
        }
    }

    #[test]
    fn id_to_filename_replaces_slashes() {
        let name = FactStore::id_to_filename("aletheia/spawn/model");
        assert_eq!(name, "aletheia-spawn-model.json");
        let name2 = FactStore::id_to_filename("aletheia.spawn.model");
        assert_eq!(name2, "aletheia.spawn.model.json");
    }
}
