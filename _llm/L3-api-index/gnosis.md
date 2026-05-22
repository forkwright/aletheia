# L3 API Index: gnosis

Crate path: `crates/gnosis`

Public API signatures. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub enum GnosisError {
    /// `cargo metadata` failed to run or returned a non-zero exit.
    #[snafu(display("cargo metadata failed: {source}"))]
    CargoMetadata { source: cargo_metadata::Error },

    /// A Rust source file could not be read.
    #[snafu(display("failed to read source file {}: {source}", path.display()))]
    ReadSource {
        path: PathBuf,
        source: std::io::Error,
    },

    /// `syn` failed to parse a Rust source file.
    #[snafu(display("failed to parse {}: {source}", path.display()))]
    Parse { path: PathBuf, source: syn::Error },

    /// A `fjall` operation failed.
    #[snafu(display("fjall error: {source}"))]
    Fjall { source: fjall::Error },

    /// Stored index data could not be encoded or decoded.
    #[snafu(display("index serialization error: {source}"))]
    Codec { source: serde_json::Error },

    /// Stored index data is malformed.
    #[snafu(display("corrupt index data: {message}"))]
    Corrupt { message: String },

    /// The index cache directory could not be created.
    #[snafu(display("failed to create cache directory {}: {source}", dir.display()))]
    CreateCacheDir {
        dir: PathBuf,
        source: std::io::Error,
    },

    /// The stale index cache file could not be removed.
    #[snafu(display("failed to remove stale cache file {}: {source}", path.display()))]
    RemoveCacheFile {
        path: PathBuf,
        source: std::io::Error,
    },
}
```

> Convenience alias.
```rust
pub type Result<T> = std::result::Result<T, GnosisError>;
```

## `src/lib.rs`

> A handle to the gnosis fjall index.
>
> # Thread safety
>
> The fjall keyspace handles are thread-safe. We keep them behind a mutex so
> query and rebuild operations see a consistent sequence of mutations.
```rust
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
```

```rust
impl CodeGraph {
    pub fn open (db_path: &Path, workspace_root: &Path) -> Result<Self>;
    pub fn open_default (workspace_root: &Path) -> Result<Self>;
    pub fn default_cache_path () -> PathBuf;
    pub fn schema_version (&self) -> u32;
    pub fn producer (&self) -> &str;
    pub fn rebuild (&self) -> Result<()>;
    pub fn symbol_rdeps (
        &self,
        symbol_name: &str,
        target_crate: Option<&str>,
    ) -> Result<Vec<QueryRow>>;
    pub fn impl_search (&self, trait_name: &str) -> Result<Vec<QueryRow>>;
    pub fn reexport_chain (&self, symbol_name: &str) -> Result<Vec<QueryRow>>;
    pub fn crate_deps (&self, crate_name: &str) -> Result<Vec<QueryRow>>;
    pub fn crate_rdeps (&self, crate_name: &str) -> Result<Vec<QueryRow>>;
    pub fn symbols_in (&self, crate_name: &str, kind: Option<&str>) -> Result<Vec<QueryRow>>;
}
```

## `src/query.rs`

```rust
pub struct QueryRow {
    /// The crate that contains this result entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crate_name: Option<String>,
    /// Module path within the crate (e.g. `"types::message"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_path: Option<String>,
    /// Symbol name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_name: Option<String>,
    /// Symbol kind (`fn`, `struct`, `enum`, `trait`, `impl`, `reexport`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_kind: Option<String>,
    /// Absolute path to the source file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// 1-based line number of the definition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_start: Option<i64>,
    /// Reference kind for edge-type results (`impl`, `reexport`).
    /// In v1 only `impl` and `reexport` edges are indexed; call-site and
    /// type-use edges are deferred to v2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_kind: Option<String>,
    /// Provenance: `"gnosis@<schema_version>"`.
    pub source: String,
}
```
