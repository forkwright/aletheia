# L3 API Index: diaporeia

Crate path: `crates/diaporeia`

Public API signatures extracted from source. Doc comments shown above each signature.
For implementation context, read the source directly (`L4`).

## `src/auth.rs`

```rust
pub struct McpClaims {
    /// Subject identifier (user or service principal).
    pub sub: String,
    /// Authorization role governing API access.
    pub role: Role,
    /// Optional nous scope: when set, restricts access to a single agent.
    pub nous_id: Option<String>,
}
```

```rust
pub async fn mcp_auth (
    state: Arc<DiaporeiaState>,
    mut req: Request<Body>,
    next: Next,
) -> Response<Body>
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
}
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
    pub fn with_state (state: Arc<DiaporeiaState>) -> Self;
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
}
```

## `src/transport.rs`

> Build an Axum Router that serves MCP over streamable HTTP.
> 
> Mount this into the main application router to expose MCP at `/mcp`.
> The auth middleware validates Bearer JWT tokens (or passes through
> anonymous claims when `auth_mode == "none"`).
> 
> # Security warnings
> 
> Logs a `WARN` when `auth_mode == "none"` (all connections receive the
> configured `none_role` without any credential check). Escalates to
> `ERROR` when the bind address is not loopback, because the MCP server
> is reachable from the network with no authentication.
```rust
pub fn streamable_http_router (state: Arc<DiaporeiaState>) -> axum::Router
```
