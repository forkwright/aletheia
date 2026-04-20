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
    #[expect(
        dead_code,
        reason = "read by #[tool_handler] macro-generated code in ServerHandler impl"
    )]
    tool_router: ToolRouter<Self>,
}
```

```rust
impl MemoryServer {
    pub fn new (store: Arc<KnowledgeStore>, store_path: Option<PathBuf>) -> Self;
    pub fn open_fjall (path: impl AsRef<Path>) -> error::Result<Self>;
    pub fn open_in_memory () -> error::Result<Self>;
    pub async fn serve_stdio (self) -> error::Result<()>;
}
```

## `src/tools.rs`

```rust
pub struct MemorySearchParams {
    /// Free-text query string; matched via BM25 against current fact content.
    pub query: String,
    /// Maximum number of results to return. Defaults to 20 when omitted.
    #[serde(default)]
    pub limit: Option<usize>,
}
```

```rust
pub struct MemoryNeighborsParams {
    /// ID of the seed fact whose entity neighbors should be returned.
    pub fact_id: String,
}
```
