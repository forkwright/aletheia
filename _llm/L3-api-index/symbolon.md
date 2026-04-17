# L3 API Index: symbolon

Crate path: `crates/symbolon`

Public API signatures extracted from source. Doc comments shown above each signature.
For implementation context, read the source directly (`L4`).

## `src/circuit_breaker.rs`

```rust
pub enum CircuitState {
    /// Normal operation: requests flow through, failures counted.
    Closed,
    /// Tripped: requests fail immediately, cooldown timer running.
    Open,
    /// Probing: one request allowed through to test recovery.
    HalfOpen,
}
```

```rust
pub struct CircuitBreakerConfig {
    /// Number of failures within the window to trip the circuit.
    pub failure_threshold: u32,
    /// Sliding window for failure counting.
    pub failure_window: Duration,
    /// Base cooldown before transitioning from Open to `HalfOpen`.
    pub cooldown: Duration,
    /// Maximum cooldown after exponential backoff.
    pub max_cooldown: Duration,
}
```

## `src/credential/device_code.rs`

```rust
pub enum DeviceCodeError {
    /// HTTP request failed.
    #[snafu(display("HTTP request failed: {source}"))]
    HttpRequest {
        /// Underlying HTTP error.
        source: reqwest::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// OAuth error response.
    #[snafu(display("OAuth error: {error}"))]
    OAuthError {
        /// OAuth error code.
        error: String,
        /// OAuth error description.
        error_description: Option<String>,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to parse response.
    #[snafu(display("failed to parse response: {source}"))]
    ParseResponse {
        /// JSON parse error.
        source: serde_json::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Device code expired before user completed authorization.
    #[snafu(display("device code expired - please try again"))]
    ExpiredToken {
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// User denied the authorization request.
    #[snafu(display("access denied by user"))]
    AccessDenied {
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The device authorization endpoint is not configured.
    #[snafu(display("device authorization endpoint not configured for this provider"))]
    MissingDeviceEndpoint {
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Polling was cancelled.
    #[snafu(display("authentication was cancelled"))]
    Cancelled {
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to save credential file.
    #[snafu(display("failed to save credentials: {source}"))]
    SaveCredential {
        /// IO error from save operation.
        source: std::io::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

```rust
pub struct DeviceOAuthProvider {
    /// Base OAuth provider configuration.
    pub base: OAuthProvider,
    /// Device authorization endpoint URL.
    pub device_authorization_url: String,
}
```

```rust
impl DeviceOAuthProvider {
    pub fn new (
        client_id: impl Into<String>,
        authorization_url: impl Into<String>,
        token_url: impl Into<String>,
        device_authorization_url: impl Into<String>,
    ) -> Self;
    pub fn with_scope (mut self, scope: impl Into<String>) -> Self;
    pub fn with_redirect_uri (mut self, uri: impl Into<String>) -> Self;
}
```

```rust
pub async fn device_code_login (provider: &DeviceOAuthProvider) -> Result<CredentialFile>
```

```rust
pub async fn device_code_login_and_save (
    provider: &DeviceOAuthProvider,
    path: &std::path::Path,
) -> Result<CredentialFile>
```

```rust
pub async fn device_code_login_with_callback <F> (
    provider: &DeviceOAuthProvider,
    display_callback: F,
) -> Result<CredentialFile> where
    F: FnOnce(&str, &str, Option<&str>),
```

## `src/credential/file_ops.rs`

```rust
pub struct CredentialFile {
    /// Access token (API key or OAuth access token).
    #[serde(alias = "accessToken", serialize_with = "serialize_secret")]
    pub token: SecretString,
    /// OAuth refresh token (absent for static API keys).
    #[serde(
        rename = "refreshToken",
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_option_secret"
    )]
    pub refresh_token: Option<SecretString>,
    /// Token expiry as milliseconds since epoch.
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    /// OAuth scopes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
    /// Subscription tier.
    #[serde(rename = "subscriptionType", skip_serializing_if = "Option::is_none")]
    pub subscription_type: Option<String>,
}
```

```rust
impl CredentialFile {
    pub fn load (path: &Path) -> Option<Self>;
    pub fn has_refresh_token (&self) -> bool;
    pub fn seconds_remaining (&self) -> Option<i64>;
}
```

## `src/credential/keyring_provider.rs`

> Reads credentials from the OS keyring (GNOME Keyring, macOS Keychain,
> Windows Credential Manager).
> 
> Falls through silently when the keyring is unavailable (headless server,
> no D-Bus session, locked keychain) so downstream providers get a chance.
```rust
pub struct KeyringCredentialProvider {
    service: String,
    username: String,
}
```

```rust
impl KeyringCredentialProvider {
    pub fn new () -> Self;
    pub fn store (&self, token: &str) -> Result<(), keyring::Error>;
    pub fn delete (&self) -> Result<(), keyring::Error>;
}
```

## `src/credential/pkce.rs`

```rust
pub enum PkceError {
    /// Failed to generate cryptographic random data.
    #[snafu(display("random generation failed"))]
    RandomGeneration {
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to bind local callback server.
    #[snafu(display("failed to bind callback server: {source}"))]
    ServerBind {
        /// Underlying IO error.
        source: std::io::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to accept incoming connection.
    #[snafu(display("failed to accept connection: {source}"))]
    AcceptConnection {
        /// Underlying IO error.
        source: std::io::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to set listener blocking mode.
    #[snafu(display("failed to set blocking mode on listener: {source}"))]
    SetBlocking {
        /// Underlying IO error.
        source: std::io::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to read HTTP request.
    #[snafu(display("failed to read request: {source}"))]
    ReadRequest {
        /// Underlying IO error.
        source: std::io::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to send HTTP response.
    #[snafu(display("failed to send response: {source}"))]
    SendResponse {
        /// Underlying IO error.
        source: std::io::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// HTTP request failed.
    #[snafu(display("HTTP request failed: {source}"))]
    HttpRequest {
        /// Underlying HTTP error.
        source: reqwest::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// OAuth error response from token endpoint.
    #[snafu(display("OAuth error: {error}"))]
    OAuthError {
        /// OAuth error code.
        error: String,
        /// OAuth error description.
        error_description: Option<String>,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Invalid state parameter (CSRF protection).
    #[snafu(display("invalid state parameter - possible CSRF attack"))]
    InvalidState {
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Missing authorization code in callback.
    #[snafu(display("missing authorization code in callback"))]
    MissingCode {
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Callback received an error.
    #[snafu(display("authorization error: {error}"))]
    AuthorizationError {
        /// OAuth error code.
        error: String,
        /// OAuth error description.
        error_description: Option<String>,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Timeout waiting for callback.
    #[snafu(display("timeout waiting for authorization callback"))]
    Timeout {
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to parse token response.
    #[snafu(display("failed to parse token response: {source}"))]
    ParseResponse {
        /// JSON parse error.
        source: serde_json::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to save credential file.
    #[snafu(display("failed to save credentials: {source}"))]
    SaveCredential {
        /// IO error from save operation.
        source: std::io::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

```rust
pub struct OAuthProvider {
    /// OAuth client identifier.
    pub client_id: String,
    /// Authorization endpoint URL.
    pub authorization_url: String,
    /// Token endpoint URL.
    pub token_url: String, // kanon:ignore RUST/plain-string-secret
    /// Requested OAuth scopes.
    pub scopes: Vec<String>,
    /// Optional redirect URI (defaults to localhost).
    pub redirect_uri: Option<String>,
}
```

```rust
impl OAuthProvider {
    pub fn new (
        client_id: impl Into<String>,
        authorization_url: impl Into<String>,
        token_url: impl Into<String>,
    ) -> Self;
    pub fn with_scope (mut self, scope: impl Into<String>) -> Self;
    pub fn with_redirect_uri (mut self, uri: impl Into<String>) -> Self;
}
```

```rust
pub async fn pkce_login (provider: &OAuthProvider) -> Result<CredentialFile>
```

```rust
pub async fn pkce_login_and_save (
    provider: &OAuthProvider,
    path: &std::path::Path,
) -> Result<CredentialFile>
```

## `src/credential/providers.rs`

> Reads a credential from an environment variable.
> 
> Automatically detects OAuth tokens by the `sk-ant-oat` prefix and
> returns [`CredentialSource::OAuth`] so callers use `Bearer` auth.
```rust
pub struct EnvCredentialProvider {
    var_name: String,
    /// Force the credential source (e.g. OAuth for `ANTHROPIC_AUTH_TOKEN`).
    force_source: Option<CredentialSource>,
}
```

```rust
impl EnvCredentialProvider {
    pub fn new (var_name: impl Into<String>) -> Self;
    pub fn with_source (var_name: impl Into<String>, source: CredentialSource) -> Self;
}
```

> Reads a credential from a JSON file on disk.
```rust
pub struct FileCredentialProvider {
    path: PathBuf,
    // NOTE: pub(crate) for test access after credential.rs → credential/mod.rs split
    pub(crate) cached: RwLock<Option<CachedFile>>,
}
```

```rust
impl FileCredentialProvider {
    pub fn new (path: PathBuf) -> Self;
}
```

> Ordered list of credential providers. First to return `Some` wins.
```rust
pub struct CredentialChain {
    providers: Vec<Box<dyn CredentialProvider>>,
    resolved_name: RwLock<String>,
}
```

```rust
impl CredentialChain {
    pub fn new (providers: Vec<Box<dyn CredentialProvider>>) -> Self;
}
```

## `src/credential/refresh.rs`

> Wraps a credential file with background OAuth token refresh.
> 
> Cleanup is registered at construction time via [`CleanupRegistry`](koina::cleanup::CleanupRegistry): the
> background task is cancelled and aborted when the provider is dropped,
> regardless of whether the drop occurs during normal execution, early
> error return, or panic unwind.
> 
> // WHY: `RwLock` allows concurrent readers (`get_credential` calls) with a
> // single writer (the background refresh task). This avoids blocking
> // LLM requests during token refresh, which may take 100-500ms.
```rust
pub struct RefreshingCredentialProvider {
    /// Current OAuth token and refresh metadata. `None` after a fatal
    /// refresh failure. Writers: the background refresh task (exclusive).
    /// Readers: `get_credential()` on any thread.
    state: Arc<RwLock<Option<RefreshState>>>,
    file_provider: FileCredentialProvider,
    shutdown: CancellationToken,
    /// Cleanup registered at task spawn time; fires on drop (LIFO order).
    _cleanup: koina::cleanup::CleanupRegistry,
}
```

```rust
impl RefreshingCredentialProvider {
    pub fn new (path: PathBuf) -> Option<Self>;
}
```

```rust
pub async fn force_refresh (path: &Path) -> Result<CredentialFile, String>
```

```rust
pub fn claude_code_default_path () -> Option<PathBuf>
```

```rust
pub fn claude_code_provider (path: &Path) -> Option<Box<dyn CredentialProvider>>
```

> Build a credential provider with a custom circuit breaker configuration.
> 
> See [`claude_code_provider`] for behavior details.
```rust
pub fn claude_code_provider_with_config (
    path: &Path,
    cb_config: CircuitBreakerConfig,
) -> Option<Box<dyn CredentialProvider>>
```

## `src/error.rs`

```rust
pub enum Error {
    /// JWT token is malformed or has an invalid signature.
    #[snafu(display("invalid token: {message}"))]
    InvalidToken {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JWT token has expired.
    #[snafu(display("token expired"))]
    ExpiredToken {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Username or password is incorrect.
    #[snafu(display("invalid credentials"))]
    InvalidCredentials {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The authenticated principal lacks permission for the requested action.
    #[snafu(display("permission denied: {role} cannot {action}"))]
    PermissionDenied {
        action: String,
        role: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Password hashing or verification failed.
    #[snafu(display("hash error: {message}"))]
    Hash {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JWT encoding failed.
    #[snafu(display("token encode error: {message}"))]
    TokenEncode {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JWT decoding failed.
    #[snafu(display("token decode error: {message}"))]
    TokenDecode {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// API key format is invalid.
    #[snafu(display("invalid API key format"))]
    InvalidApiKey {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Entity not found.
    #[snafu(display("{entity} not found: {id}"))]
    NotFound {
        entity: String,
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Duplicate entity.
    #[snafu(display("duplicate {entity}: {id}"))]
    Duplicate {
        entity: String,
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JWT signing key is the insecure default placeholder.
    #[snafu(display(
        "insecure JWT signing key: default placeholder active with auth mode '{auth_mode}'. Set auth.jwt_secret in config or the ALETHEIA_JWT_SECRET env var. Generate one with: openssl rand -hex 32"
    ))]
    InsecureKey {
        auth_mode: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Storage backend operation failed.
    ///
    /// Used by the fjall backend for LSM-tree and JSON encoding errors.
    #[snafu(display("storage error: {message}"))]
    Storage {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Filesystem I/O error.
    ///
    /// Used by the fjall backend when creating the store directory.
    #[snafu(display("I/O error at {}: {source}", path.display()))]
    Io {
        path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

## `src/jwt.rs`

> Default clock skew leeway applied to JWT expiration checks.
> 
> WHY: clock drift between the issuer and validator (or NTP jumps on the
> validator) can immediately invalidate freshly issued tokens. 30s is
> small enough that truly expired tokens are rejected in practice while
> tolerating typical NTP drift. Mirrors the tolerance used by the OAuth
> credential chain and by `pylon::handlers::health`.
```rust
pub const DEFAULT_CLOCK_SKEW_LEEWAY_SECS: u64 = 30;
```

> Configuration for JWT token management.
```rust
pub struct JwtConfig {
    /// HMAC-SHA256 signing key.
    pub signing_key: SecretString,
    /// Access token time-to-live (default: 1 hour).
    pub access_ttl: Duration,
    /// Refresh token time-to-live (default: 7 days).
    pub refresh_ttl: Duration,
    /// Issuer claim value.
    pub issuer: String,
    /// Clock skew tolerance (seconds) applied when checking `exp`.
    ///
    /// A token whose `exp` lies up to `clock_skew_leeway_secs` seconds in
    /// the past is still accepted. Default:
    /// [`DEFAULT_CLOCK_SKEW_LEEWAY_SECS`] (30s).
    pub clock_skew_leeway_secs: u64,
}
```

```rust
impl JwtConfig {
    pub fn validate_for_auth_mode (&self, auth_mode: &str) -> Result<()>;
}
```

> Manages JWT issuance and validation.
```rust
pub struct JwtManager {
    /// Raw secret bytes for HMAC-SHA256 signing.
    signing_key_bytes: Vec<u8>,
    config: JwtConfig,
}
```

```rust
impl JwtManager {
    pub fn new (config: JwtConfig) -> Self;
    pub fn issue_access (&self, sub: &str, role: Role, nous_id: Option<&str>) -> Result<String>;
    pub fn issue_refresh (&self, sub: &str, role: Role) -> Result<String>;
    pub fn validate (&self, token: &str) -> Result<Claims>;
    pub fn encode_claims (&self, claims: &Claims) -> Result<String>;
}
```

## `src/metrics.rs`

> Register this crate's metrics with the shared registry.
```rust
pub fn register (registry: &mut Registry)
```

## `src/types.rs`

```rust
pub enum Role {
    /// Dashboard access only. No mutations.
    Readonly,
    /// Per-nous scoped. Can access own sessions, use own tools, read shared workspace.
    Agent,
    /// Full access. Can manage agents, users, read all sessions, configure system.
    Operator,
    /// Superuser. All Operator permissions plus system administration.
    Admin,
}
```

```rust
pub enum TokenKind {
    /// Short-lived token for API access.
    Access,
    /// Long-lived token used to obtain new access tokens.
    Refresh,
}
```

```rust
pub struct Claims {
    /// Subject: user or agent ID.
    pub sub: String,
    /// RBAC role.
    pub role: Role,
    /// For agent tokens, the nous ID this token is scoped to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nous_id: Option<String>,
    /// Issuer.
    pub iss: String,
    /// Issued-at (unix seconds).
    pub iat: i64,
    /// Expiration (unix seconds).
    pub exp: i64,
    /// Unique token ID (for revocation).
    pub jti: String,
    /// Access or refresh.
    pub kind: TokenKind,
}
```
