# L3 API Index: diaporeia

Crate path: `crates/diaporeia`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/auth.rs`

```rust
pub async fn mcp_auth (
    state: Arc<DiaporeiaState>,
    req: Request<Body>,
    next: Next,
) -> Response<Body>
```

## `src/client.rs`

```rust
pub struct StdioMcpServerConfig {
    /// Executable to spawn.
    pub command: String,
    /// Command-line arguments passed to the executable.
    pub args: Vec<String>,
    /// Optional working directory for the child process.
    pub cwd: Option<PathBuf>,
    /// Extra environment variables for the child process.
    pub env: HashMap<String, String>,
}
```

```rust
impl StdioMcpServerConfig {
    pub fn new (command: impl Into<String>) -> Self;
}
```

```rust
pub struct ExternalMcpClient {
    service: Arc<Mutex<RunningService<RoleClient, ClientInfo>>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern -- WHY: tokio::sync::Mutex is used (correct for async); not std::sync::Mutex
}
```

```rust
impl ExternalMcpClient {
    pub async fn connect_stdio (config: &StdioMcpServerConfig) -> Result<Self>;
    pub async fn connect_streamable_http (endpoint: &str) -> Result<Self>;
    pub async fn list_tools (&self) -> Result<Vec<Tool>>;
    pub async fn call_tool (
        &self,
        name: &str,
        arguments: serde_json::Map<String, serde_json::Value>,
    ) -> Result<CallToolResult>;
    pub async fn close (&self) -> Result<QuitReason>;
}
```

## `src/error.rs`

```rust
pub enum Error {
    /// Nous agent not found.
    #[snafu(display("nous agent not found: {id}"))]
    NousNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session not found.
    #[snafu(display("session not found: {id}"))]
    SessionNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A session with this (nous_id, session_key) pair already exists.
    #[snafu(display("a session with key '{session_key}' already exists for agent '{nous_id}'"))]
    DuplicateSession {
        nous_id: String,
        session_key: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Nous pipeline error.
    #[snafu(display("nous pipeline error: {message}"))]
    Pipeline {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session store error.
    #[snafu(display("session store error: {source}"))]
    SessionStore {
        source: mneme::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Serialization error.
    #[snafu(display("serialization error: {source}"))]
    Serialization {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Transport error.
    #[snafu(display("transport error: {message}"))]
    Transport {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// I/O error reading a workspace file.
    #[snafu(display("workspace file error: {source}"))]
    WorkspaceFile {
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Caller lacks the required role for this operation.
    #[snafu(display("unauthorized: {message}"))]
    Unauthorized {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The knowledge graph MCP surface is disabled or not configured.
    #[snafu(display(
        "knowledge graph MCP surface is disabled; \
         set mcp.knowledge_graph.enabled = true in aletheia.toml"
    ))]
    KnowledgeStoreUnavailable {
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

    /// The requested fact was not found.
    #[snafu(display("fact not found: {id}"))]
    FactNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The caller supplied an invalid input value.
    #[snafu(display("invalid input: {message}"))]
    InvalidInput {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The session notes MCP surface is disabled or not configured.
    #[snafu(display("session notes store is not available"))]
    NoteStoreUnavailable {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The shared blackboard MCP surface is disabled or not configured.
    #[snafu(display("blackboard store is not available"))]
    BlackboardStoreUnavailable {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A note store operation failed.
    #[snafu(display("note store error: {message}"))]
    NoteStore {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A blackboard store operation failed.
    #[snafu(display("blackboard store error: {message}"))]
    BlackboardStore {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The repomix MCP surface is disabled or not configured.
    #[snafu(display(
        "repomix MCP surface is disabled; set mcp.repomix.enabled = true in aletheia.toml"
    ))]
    RepomixUnavailable {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The requested repomix template was not found.
    #[snafu(display("repomix template not found: {name}"))]
    TemplateNotFound {
        name: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A repomix pack operation failed.
    #[snafu(display("repomix pack error: {message}"))]
    RepomixPack {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

> Result alias using diaporeia's [`Error`] type.
```rust
pub type Result<T> = std::result::Result<T, Error>;
```

## `src/rate_limit.rs`

> Per-session rate limiter with separate buckets for expensive and cheap operations.
```rust
pub struct RateLimiter {
    expensive: TokenBucket,
    cheap: TokenBucket,
    enabled: bool,
}
```

## `src/server.rs`

```rust
pub struct DiaporeiaServer {
    pub(crate) state: Arc<DiaporeiaState>,
    pub(crate) rate_limiter: Arc<RateLimiter>,
    #[expect(
        dead_code,
        reason = "read by #[tool_handler] macro-generated code in ServerHandler impl"
    )]
    tool_router: ToolRouter<Self>,
}
```

```rust
impl DiaporeiaServer {
    pub fn with_state (state: Arc<DiaporeiaState>, rate_cfg: &McpRateLimitConfig) -> Self;
}
```

## `src/state.rs`

> Shared state for the diaporeia MCP server.
> 
> Holds the same shared `Arc` pointers as pylon's `AppState`.
> Both live in the same process and access identical instances.
```rust
pub struct DiaporeiaState {
    /// Persistent session and message storage.
    pub session_store: Arc<Mutex<SessionStore>>,
    /// Manages nous actor lifecycles and provides handles.
    pub nous_manager: Arc<NousManager>,
    /// Registry of tools available to nous agents.
    pub tool_registry: Arc<ToolRegistry>,
    /// Instance directory layout for file resolution.
    pub oikos: Arc<Oikos>,
    /// JWT token validation (shared with pylon).
    ///
    /// `None` when `auth_mode == "none"` (no signing key available).
    pub jwt_manager: Option<Arc<JwtManager>>,
    /// Server start instant for uptime calculation.
    pub start_time: Instant,
    /// Runtime configuration, updatable via config API.
    pub config: Arc<tokio::sync::RwLock<AletheiaConfig>>,
    /// Auth mode from gateway config (`"token"`, `"none"`, etc.).
    pub auth_mode: String,
    /// Role assigned to anonymous requests when auth mode is `"none"`.
    pub none_role: String,
    /// Root shutdown token.
    pub shutdown: CancellationToken,
    /// Shared knowledge store for the knowledge graph MCP surface.
    ///
    /// `None` when the knowledge store is not configured or when
    /// `mcp.knowledge_graph.enabled` is `false`.
    #[cfg(feature = "knowledge-store")]
    pub knowledge_store: Option<Arc<KnowledgeStore>>,
    /// Session notes store for the `memory_note` tool.
    pub note_store: Option<Arc<dyn NoteStore>>,
    /// Shared blackboard store for the `memory_blackboard` tool.
    pub blackboard_store: Option<Arc<dyn BlackboardStore>>,
}
```

## `src/transport.rs`

> Build an Axum Router that serves MCP over streamable HTTP.
> 
> Mount this into the main application router to expose MCP at `/mcp`.
> The auth middleware validates Bearer JWT tokens (or passes through
> anonymous claims when `auth_mode == "none"`).
> 
> Delegates to [`streamable_http_router_with_config`] with the default
> `StreamableHttpServerConfig`. See that function for security warning details.
```rust
pub fn streamable_http_router (state: Arc<DiaporeiaState>) -> axum::Router
```

> Build an Axum Router with a custom Streamable HTTP config.
> 
> Used by integration tests to enable stateless+json-response mode for
> simpler request-response testing without SSE parsing.
> 
> # Security warnings
> 
> Logs a `WARN` when `auth_mode == "none"` (all connections receive the
> configured `none_role` without any credential check). Escalates to
> `ERROR` when the bind address is not loopback, because the MCP server
> is reachable from the network with no authentication.
```rust
pub fn streamable_http_router_with_config (
    state: Arc<DiaporeiaState>,
    config: rmcp::transport::streamable_http_server::StreamableHttpServerConfig,
) -> axum::Router
```

```rust
pub async fn serve_stdio (state: Arc<DiaporeiaState>) -> Result<()>
```

## `tests/common/mod.rs`

```rust
pub struct StateBuilder {
    auth_mode: String,
    none_role: String,
    signing_key: String,
    instance_root: tempfile::TempDir,
    repomix_enabled: bool,
}
```

```rust
impl StateBuilder {
    pub fn new () -> Self;
    pub fn auth_mode (mut self, mode: &str) -> Self;
    pub fn none_role (mut self, role: &str) -> Self;
    pub fn repomix_enabled (mut self) -> Self;
    pub fn build (self) -> (Arc<DiaporeiaState>, Arc<JwtManager>, tempfile::TempDir);
}
```

```rust
pub fn issue_token (jwt: &JwtManager, subject: &str, role: Role) -> String
```
