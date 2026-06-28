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
//! (default: `~/aletheia/instance/facts/`).  Each fact is serialised to a
//! collision-checked safe filename derived from its id: short ids use
//! percent-encoded UTF-8 bytes, while ids too long for common filename limits
//! use a SHA-256 stem.  The original id remains inside the JSON payload. No
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

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::io;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use snafu::{ResultExt, Snafu};
use tokio::sync::Mutex;

const ENCODED_FILENAME_PREFIX: &str = "id-";
const HASH_FILENAME_PREFIX: &str = "hash-";
const JSON_SUFFIX: &str = ".json";
const MAX_SAFE_FILENAME_BYTES: usize = 240;

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

    /// A mapped fact file contains a different embedded id.
    #[snafu(display(
        "fact id {requested_id} maps to {}, but that file stores fact id {stored_id}",
        path.display()
    ))]
    FilenameCollision {
        path: PathBuf,
        requested_id: String,
        stored_id: String,
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
/// One file per fact: `<dir>/<safe_id>.json` where `<safe_id>` is a
/// collision-checked filename derived from the fact's `id`.
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
        let encoded = Self::percent_encode_id(id);
        let encoded_len = ENCODED_FILENAME_PREFIX.len() + encoded.len() + JSON_SUFFIX.len();
        if encoded_len <= MAX_SAFE_FILENAME_BYTES {
            format!("{ENCODED_FILENAME_PREFIX}{encoded}{JSON_SUFFIX}")
        } else {
            format!(
                "{HASH_FILENAME_PREFIX}{}{JSON_SUFFIX}",
                Self::sha256_hex(id)
            )
        }
    }

    fn legacy_id_to_filename(id: &str) -> String {
        let mut name: String = id
            .chars()
            .map(|c| match c {
                '/' | '\\' => '-',
                _ => c,
            })
            .collect();
        name.push_str(JSON_SUFFIX);
        name
    }

    fn percent_encode_id(id: &str) -> String {
        let mut encoded = String::with_capacity(id.len());
        for byte in id.bytes() {
            if Self::is_filename_literal(byte) {
                encoded.push(char::from(byte));
            } else {
                encoded.push('%');
                encoded.push(Self::upper_hex_digit(byte >> 4));
                encoded.push(Self::upper_hex_digit(byte & 0x0f));
            }
        }
        encoded
    }

    fn is_filename_literal(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')
    }

    fn sha256_hex(id: &str) -> String {
        let digest = Sha256::digest(id.as_bytes());
        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            hex.push(Self::lower_hex_digit(byte >> 4));
            hex.push(Self::lower_hex_digit(byte & 0x0f));
        }
        hex
    }

    fn upper_hex_digit(nibble: u8) -> char {
        match nibble {
            0..=9 => char::from(b'0' + nibble),
            10..=15 => char::from(b'A' + (nibble - 10)),
            _ => '?',
        }
    }

    fn lower_hex_digit(nibble: u8) -> char {
        match nibble {
            0..=9 => char::from(b'0' + nibble),
            10..=15 => char::from(b'a' + (nibble - 10)),
            _ => '?',
        }
    }

    fn fact_path(&self, id: &str) -> PathBuf {
        self.dir.join(Self::id_to_filename(id))
    }

    fn legacy_fact_path(&self, id: &str) -> PathBuf {
        self.dir.join(Self::legacy_id_to_filename(id))
    }

    async fn fact_path_exists(path: &Path) -> Result<bool, FactError> {
        tokio::fs::try_exists(path)
            .await
            .with_context(|_| ReadFileSnafu {
                path: path.to_path_buf(),
            })
    }

    async fn read_fact_file(path: &Path) -> Result<ArchitectureFact, FactError> {
        let bytes = tokio::fs::read(path)
            .await
            .with_context(|_| ReadFileSnafu {
                path: path.to_path_buf(),
            })?;
        serde_json::from_slice(&bytes).with_context(|_| DeserialiseSnafu {
            path: path.to_path_buf(),
        })
    }

    fn require_embedded_id(
        path: &Path,
        requested_id: &str,
        fact: ArchitectureFact,
    ) -> Result<ArchitectureFact, FactError> {
        if fact.id == requested_id {
            return Ok(fact);
        }
        FilenameCollisionSnafu {
            path: path.to_path_buf(),
            requested_id: requested_id.to_owned(),
            stored_id: fact.id,
        }
        .fail()
    }

    async fn read_fact_for_id(path: &Path, id: &str) -> Result<ArchitectureFact, FactError> {
        let fact = Self::read_fact_file(path).await?;
        Self::require_embedded_id(path, id, fact)
    }

    async fn require_path_available_for_id(path: &Path, id: &str) -> Result<(), FactError> {
        if !Self::fact_path_exists(path).await? {
            return Ok(());
        }
        let fact = Self::read_fact_file(path).await?;
        Self::require_embedded_id(path, id, fact).map(|_| ())
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
        if Self::fact_path_exists(&path).await? {
            return Self::read_fact_for_id(&path, id).await.map(Some);
        }

        let legacy_path = self.legacy_fact_path(id);
        if legacy_path != path && Self::fact_path_exists(&legacy_path).await? {
            return Self::read_fact_for_id(&legacy_path, id).await.map(Some);
        }

        Ok(None)
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
        Self::require_path_available_for_id(&path, &fact.id).await?;
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
        let mut facts = BTreeMap::new();
        while let Some(entry) = entries.next_entry().await.with_context(|_| DirEntrySnafu {
            dir: self.dir.clone(),
        })? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let fact = Self::read_fact_file(&path).await?;
            let filename = path.file_name().and_then(|name| name.to_str());
            let canonical = Self::id_to_filename(&fact.id);
            let is_canonical = filename == Some(canonical.as_str());
            match facts.entry(fact.id.clone()) {
                Entry::Vacant(entry) => {
                    entry.insert((is_canonical, fact));
                }
                Entry::Occupied(mut entry) if is_canonical && !entry.get().0 => {
                    entry.insert((true, fact));
                }
                Entry::Occupied(_) => {}
            }
        }
        Ok(facts.into_values().map(|(_, fact)| fact).collect())
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
#[path = "architecture_fact_tests.rs"]
mod tests;
