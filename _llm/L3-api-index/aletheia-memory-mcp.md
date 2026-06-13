# L3 API Index: aletheia-memory-mcp

Crate path: `crates/aletheia-memory-mcp`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub enum Error {
    /// Failed to open the knowledge store.
    #[snafu(display("failed to open knowledge store: {message}"))]
    OpenStore {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A knowledge store operation failed.
    #[snafu(display("knowledge store error: {message}"))]
    KnowledgeStore {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Serialization of a response payload failed.
    #[snafu(display("serialization error: {source}"))]
    Serialization {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Caller supplied an invalid input value.
    #[snafu(display("invalid input: {message}"))]
    InvalidInput {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// MCP transport error (stdio read/write or shutdown).
    #[snafu(display("transport error: {message}"))]
    Transport {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Background task join failure.
    #[snafu(display("task join error: {source}"))]
    Join {
        source: tokio::task::JoinError,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Write tool invoked without capability token configured.
    #[snafu(display("write tools are not available; capability token not configured"))]
    WriteNotAvailable {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Write call rejected due to invalid capability token.
    #[snafu(display("write authorization failed"))]
    WriteUnauthorized {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Fact not found for a write operation.
    #[snafu(display("fact not found: {id}"))]
    FactNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

> Result alias using this crate's [`Error`].
```rust
pub type Result<T> = std::result::Result<T, Error>;
```

## `src/server.rs`

```rust
pub struct MemoryServer {
    pub(crate) store: Arc<KnowledgeStore>,
    pub(crate) store_path: Option<PathBuf>,
    /// Capability token for write tools, if configured.
    /// If `None`, write tools are not registered.
    pub(crate) write_token: Option<String>,
    tool_router: ToolRouter<Self>,
}
```

```rust
impl MemoryServer {
    pub fn new (store: Arc<KnowledgeStore>, store_path: Option<PathBuf>) -> Self;
    pub fn with_write_token (
        store: Arc<KnowledgeStore>,
        store_path: Option<PathBuf>,
        write_token: Option<String>,
    ) -> Self;
    pub fn open_fjall (path: impl AsRef<Path>) -> error::Result<Self>;
    pub fn open_in_memory () -> error::Result<Self>;
    pub async fn serve_stdio (self) -> error::Result<()>;
}
```

## `src/tools.rs`

```rust
pub struct NousSearchParams {
    /// Free-text query string; matched via BM25 against current fact content.
    pub query: String,
    /// Owning agent (nous) for whom results are being recalled. Filters out
    /// foreign private facts and respects visibility rules.
    pub nous_id: String,
    /// Maximum number of results to return. Defaults to 20 when omitted.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Optional project partition (64-character SHA-256 hex) to restrict results.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Optional memory scope (`user`, `feedback`, `project`, or `reference`).
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional minimum visibility (`private`, `shared`, `restricted`, `published`).
    #[serde(default)]
    pub min_visibility: Option<String>,
    /// Optional maximum sensitivity (`public`, `internal`, `confidential`).
    #[serde(default)]
    pub max_sensitivity: Option<String>,
}
```

```rust
pub struct NousListTopicsParams {
    /// Owning agent (nous) whose view of the topic distribution is requested.
    pub nous_id: String,
    /// Optional project partition filter.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Optional memory scope filter.
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional minimum visibility filter.
    #[serde(default)]
    pub min_visibility: Option<String>,
    /// Optional maximum sensitivity filter.
    #[serde(default)]
    pub max_sensitivity: Option<String>,
}
```

```rust
pub struct NousStatsParams {
    /// Owning agent (nous) whose view of the stats is requested.
    pub nous_id: String,
    /// Optional project partition filter.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Optional memory scope filter.
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional minimum visibility filter.
    #[serde(default)]
    pub min_visibility: Option<String>,
    /// Optional maximum sensitivity filter.
    #[serde(default)]
    pub max_sensitivity: Option<String>,
}
```

```rust
pub struct NousNeighborsParams {
    /// ID of the seed fact whose entity neighbors should be returned.
    // kanon:ignore RUST/primitive-for-domain-id — WHY: MCP JSON protocol boundary; String required for serde/schemars JsonSchema derivation
    pub fact_id: String,
    /// Owning agent (nous) whose scoped view is requesting the neighbors.
    pub nous_id: String,
    /// Optional project partition filter.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Optional memory scope filter.
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional minimum visibility filter.
    #[serde(default)]
    pub min_visibility: Option<String>,
    /// Optional maximum sensitivity filter.
    #[serde(default)]
    pub max_sensitivity: Option<String>,
}
```

```rust
pub struct NousAnnotateParams {
    /// Owning agent (nous) that is authoring the annotation. Must be explicit;
    /// the `mcp-client` fallback is no longer used for user memory.
    pub session_id: String,
    /// Fact ID to annotate.
    // kanon:ignore RUST/primitive-for-domain-id — WHY: MCP JSON protocol boundary; String required for serde/schemars JsonSchema derivation
    pub fact_id: String,
    /// Annotation content — agent-authored note or observation.
    pub content: String,
    /// Capability token for write authorization.
    pub write_token: String,
}
```

```rust
pub struct NousSupersedeParams {
    /// ID of the fact being superseded.
    // kanon:ignore RUST/primitive-for-domain-id — WHY: MCP JSON protocol boundary; String required for serde/schemars JsonSchema derivation
    pub old_fact_id: String,
    /// ID of the new fact that supersedes it.
    // kanon:ignore RUST/primitive-for-domain-id — WHY: MCP JSON protocol boundary; String required for serde/schemars JsonSchema derivation
    pub new_fact_id: String,
    /// Owning agent (nous) recording the supersession. Must be explicit.
    pub nous_id: String,
    /// Reason for supersession.
    pub reason: String,
    /// Capability token for write authorization.
    pub write_token: String,
}
```

```rust
pub struct NousForgetParams {
    /// ID of the fact to forget.
    // kanon:ignore RUST/primitive-for-domain-id — WHY: MCP JSON protocol boundary; String required for serde/schemars JsonSchema derivation
    pub fact_id: String,
    /// Owning agent (nous) requesting the forget. Must match the fact owner.
    pub nous_id: String,
    /// Reason for forgetting.
    pub reason: String,
    /// Capability token for write authorization.
    pub write_token: String,
}
```
