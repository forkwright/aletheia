//! fjall schema for the gnosis code-graph index.
//!
//! # Keyspaces
//!
//! - `symbols` — one JSON row per public-ish definition.
//! - `symbol_refs` — directed edges from one symbol site to a named target
//!   (`impl`, `reexport` in v1; call-site and type-use edges are deferred to v2).
//! - `crate_edges` — workspace-level crate dependency edges, loaded from
//!   `cargo metadata` and used for `crate_deps` queries.
//! - `file_hashes` — SHA-256 of each indexed file for incremental rebuilds.
//! - `meta` — schema version and monotonic ID counters.
//!
//! # Schema version
//!
//! Stored under `meta/schema_version`. Currently `1`. Bump when the on-disk
//! shape changes in a backward-incompatible way; `CodeGraph::open` clears the
//! index when it finds a mismatch.

use std::path::Path;

use fjall::{Database, Keyspace, KeyspaceCreateOptions, PersistMode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use snafu::ResultExt;

use crate::error::{CodecSnafu, CorruptSnafu, FjallSnafu, Result};

/// Schema version embedded in fjall metadata.
pub(crate) const SCHEMA_VERSION: u32 = 1;

const META_SCHEMA_VERSION: &[u8] = b"schema_version";
const META_NEXT_SYMBOL_ID: &[u8] = b"next_symbol_id";
const META_NEXT_REF_ID: &[u8] = b"next_ref_id";

/// Persisted symbol row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SymbolRecord {
    /// Monotonic row ID.
    pub id: u64,
    /// Crate that contains the symbol.
    pub crate_name: String,
    /// Module path within the crate.
    pub module_path: String,
    /// Symbol name.
    pub symbol_name: String,
    /// Symbol kind (`fn`, `struct`, `impl`, `reexport`, ...).
    pub symbol_kind: String,
    /// Absolute source path.
    pub file_path: String,
    /// 1-based source line.
    pub line_start: i64,
}

/// Persisted symbol reference row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SymbolRefRecord {
    /// Monotonic row ID.
    pub id: u64,
    /// Source symbol ID.
    pub from_symbol: u64,
    /// Target crate.
    pub to_crate: String,
    /// Target module path.
    pub to_module: String,
    /// Target symbol.
    pub to_symbol: String,
    /// Reference kind (`impl`, `reexport`).
    pub ref_kind: String,
}

/// fjall-backed gnosis store.
pub(crate) struct Store {
    db: Database,
    symbols: Keyspace,
    refs: Keyspace,
    crate_edges: Keyspace,
    file_hashes: Keyspace,
    meta: Keyspace,
}

impl Store {
    /// Open or create the gnosis store.
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let db = Database::builder(path).open().context(FjallSnafu)?;
        let store = Self {
            symbols: db
                .keyspace("symbols", KeyspaceCreateOptions::default)
                .context(FjallSnafu)?,
            refs: db
                .keyspace("symbol_refs", KeyspaceCreateOptions::default)
                .context(FjallSnafu)?,
            crate_edges: db
                .keyspace("crate_edges", KeyspaceCreateOptions::default)
                .context(FjallSnafu)?,
            file_hashes: db
                .keyspace("file_hashes", KeyspaceCreateOptions::default)
                .context(FjallSnafu)?,
            meta: db
                .keyspace("meta", KeyspaceCreateOptions::default)
                .context(FjallSnafu)?,
            db,
        };
        store.ensure_schema()?;
        Ok(store)
    }

    fn ensure_schema(&self) -> Result<()> {
        let stored = self.meta_u32(META_SCHEMA_VERSION)?;
        if stored.is_some_and(|version| version != SCHEMA_VERSION) {
            self.clear_all()?;
        }
        self.put_meta_u32(META_SCHEMA_VERSION, SCHEMA_VERSION)?;
        if self.meta_u64(META_NEXT_SYMBOL_ID)?.is_none() {
            self.put_meta_u64(META_NEXT_SYMBOL_ID, 1)?;
        }
        if self.meta_u64(META_NEXT_REF_ID)?.is_none() {
            self.put_meta_u64(META_NEXT_REF_ID, 1)?;
        }
        self.persist()
    }

    fn clear_all(&self) -> Result<()> {
        self.symbols.clear().context(FjallSnafu)?;
        self.refs.clear().context(FjallSnafu)?;
        self.crate_edges.clear().context(FjallSnafu)?;
        self.file_hashes.clear().context(FjallSnafu)?;
        self.meta.clear().context(FjallSnafu)?;
        Ok(())
    }

    /// Persist pending writes.
    pub(crate) fn persist(&self) -> Result<()> {
        self.db.persist(PersistMode::SyncAll).context(FjallSnafu)
    }

    /// Current stored schema version.
    pub(crate) fn schema_version(&self) -> Result<u32> {
        self.meta_u32(META_SCHEMA_VERSION)?.ok_or_else(|| {
            CorruptSnafu {
                message: "missing gnosis schema version".to_owned(),
            }
            .build()
        })
    }

    /// Insert a symbol and return its ID.
    pub(crate) fn insert_symbol(
        &self,
        crate_name: &str,
        module_path: &str,
        symbol_name: &str,
        symbol_kind: &str,
        file_path: &str,
        line_start: i64,
    ) -> Result<u64> {
        let id = self.next_id(META_NEXT_SYMBOL_ID)?;
        let record = SymbolRecord {
            id,
            crate_name: crate_name.to_owned(),
            module_path: module_path.to_owned(),
            symbol_name: symbol_name.to_owned(),
            symbol_kind: symbol_kind.to_owned(),
            file_path: file_path.to_owned(),
            line_start,
        };
        self.symbols
            .insert(id_key(id), encode(&record)?)
            .context(FjallSnafu)?;
        Ok(id)
    }

    #[cfg(test)]
    /// NOTE: Set the next generated symbol ID for regression fixtures.
    pub(crate) fn set_next_symbol_id_for_test(&self, id: u64) -> Result<()> {
        self.put_meta_u64(META_NEXT_SYMBOL_ID, id)
    }

    /// Insert a symbol reference.
    pub(crate) fn insert_ref(
        &self,
        from_symbol: u64,
        to_crate: &str,
        to_module: &str,
        to_symbol: &str,
        ref_kind: &str,
    ) -> Result<()> {
        let id = self.next_id(META_NEXT_REF_ID)?;
        let record = SymbolRefRecord {
            id,
            from_symbol,
            to_crate: to_crate.to_owned(),
            to_module: to_module.to_owned(),
            to_symbol: to_symbol.to_owned(),
            ref_kind: ref_kind.to_owned(),
        };
        self.refs
            .insert(id_key(id), encode(&record)?)
            .context(FjallSnafu)
    }

    /// Delete symbols for a source file and any refs from those symbols.
    pub(crate) fn delete_symbols_for_file(&self, file_path: &str) -> Result<()> {
        let deleted_ids: Vec<u64> = self
            .symbols()?
            .into_iter()
            .filter(|symbol| symbol.file_path == file_path)
            .map(|symbol| symbol.id)
            .collect();
        for id in &deleted_ids {
            self.symbols.remove(id_key(*id)).context(FjallSnafu)?;
        }
        for reference in self.refs()? {
            if deleted_ids.contains(&reference.from_symbol) {
                self.refs.remove(id_key(reference.id)).context(FjallSnafu)?;
            }
        }
        Ok(())
    }

    /// Insert a workspace crate edge.
    pub(crate) fn insert_crate_edge(&self, from_crate: &str, to_crate: &str) -> Result<()> {
        self.crate_edges
            .insert(edge_key(from_crate, to_crate), b"1".as_slice())
            .context(FjallSnafu)
    }

    /// Clear all crate edges.
    pub(crate) fn clear_crate_edges(&self) -> Result<()> {
        self.crate_edges.clear().context(FjallSnafu)
    }

    /// Return all crate edges.
    pub(crate) fn crate_edges(&self) -> Result<Vec<(String, String)>> {
        self.crate_edges
            .iter()
            .map(|entry| {
                let (key, _value) = entry.into_inner().context(FjallSnafu)?;
                decode_edge_key(key.as_ref())
            })
            .collect()
    }

    /// Return all symbol rows.
    pub(crate) fn symbols(&self) -> Result<Vec<SymbolRecord>> {
        collect_json(&self.symbols)
    }

    /// Return a symbol by ID.
    pub(crate) fn symbol(&self, id: u64) -> Result<Option<SymbolRecord>> {
        self.symbols
            .get(id_key(id))
            .context(FjallSnafu)?
            .map(|bytes| decode(bytes.as_ref()))
            .transpose()
    }

    /// Return all reference rows.
    pub(crate) fn refs(&self) -> Result<Vec<SymbolRefRecord>> {
        collect_json(&self.refs)
    }

    /// Return the stored hash for a file.
    pub(crate) fn file_hash(&self, file_path: &str) -> Result<Option<String>> {
        let hash = self
            .file_hashes
            .get(file_path.as_bytes())
            .context(FjallSnafu)?
            .map(|bytes| String::from_utf8_lossy(bytes.as_ref()).into_owned());
        Ok(hash)
    }

    /// Store the hash for a file.
    pub(crate) fn set_file_hash(&self, file_path: &str, hash: &str) -> Result<()> {
        self.file_hashes
            .insert(file_path.as_bytes(), hash.as_bytes())
            .context(FjallSnafu)
    }

    /// Prune hashes for files no longer present.
    pub(crate) fn prune_file_hashes_not_in(&self, present: &[String]) -> Result<()> {
        let present: std::collections::BTreeSet<&str> =
            present.iter().map(String::as_str).collect();
        let paths: Vec<String> = self
            .file_hashes
            .iter()
            .map(|entry| {
                let (key, _value) = entry.into_inner().context(FjallSnafu)?;
                Ok(String::from_utf8_lossy(key.as_ref()).into_owned())
            })
            .collect::<Result<_>>()?;
        for path in paths {
            if !present.contains(path.as_str()) {
                self.file_hashes
                    .remove(path.as_bytes())
                    .context(FjallSnafu)?;
            }
        }
        Ok(())
    }

    fn next_id(&self, key: &[u8]) -> Result<u64> {
        let id = self.meta_u64(key)?.unwrap_or(1);
        self.put_meta_u64(key, id.saturating_add(1))?;
        Ok(id)
    }

    fn meta_u32(&self, key: &[u8]) -> Result<Option<u32>> {
        self.meta
            .get(key)
            .context(FjallSnafu)?
            .map(|bytes| decode(bytes.as_ref()))
            .transpose()
    }

    fn meta_u64(&self, key: &[u8]) -> Result<Option<u64>> {
        self.meta
            .get(key)
            .context(FjallSnafu)?
            .map(|bytes| decode(bytes.as_ref()))
            .transpose()
    }

    fn put_meta_u32(&self, key: &[u8], value: u32) -> Result<()> {
        self.meta.insert(key, encode(&value)?).context(FjallSnafu)
    }

    fn put_meta_u64(&self, key: &[u8], value: u64) -> Result<()> {
        self.meta.insert(key, encode(&value)?).context(FjallSnafu)
    }
}

fn collect_json<T: DeserializeOwned>(keyspace: &Keyspace) -> Result<Vec<T>> {
    keyspace
        .iter()
        .map(|entry| {
            let (_key, value) = entry.into_inner().context(FjallSnafu)?;
            decode(value.as_ref())
        })
        .collect()
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).context(CodecSnafu)
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).context(CodecSnafu)
}

fn id_key(id: u64) -> [u8; 8] {
    id.to_be_bytes()
}

fn edge_key(from_crate: &str, to_crate: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(from_crate.len() + to_crate.len() + 1);
    key.extend_from_slice(from_crate.as_bytes());
    key.push(0);
    key.extend_from_slice(to_crate.as_bytes());
    key
}

fn decode_edge_key(key: &[u8]) -> Result<(String, String)> {
    let Some(pos) = key.iter().position(|b| *b == 0) else {
        return CorruptSnafu {
            message: "malformed crate edge key".to_owned(),
        }
        .fail();
    };
    let Some(from_bytes) = key.get(..pos) else {
        return CorruptSnafu {
            message: "malformed crate edge prefix".to_owned(),
        }
        .fail();
    };
    let Some(to_bytes) = key.get(pos.saturating_add(1)..) else {
        return CorruptSnafu {
            message: "malformed crate edge suffix".to_owned(),
        }
        .fail();
    };
    let from = String::from_utf8_lossy(from_bytes).into_owned();
    let to = String::from_utf8_lossy(to_bytes).into_owned();
    Ok((from, to))
}
