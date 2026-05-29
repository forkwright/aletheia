# L3 API Index: koina

Crate path: `crates/koina`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/base64.rs`

```rust
pub enum DecodeError {
    /// Input contained a character not in the base64 alphabet.
    #[snafu(display("invalid base64 character: {ch} at position {position}"))]
    InvalidChar {
        /// The offending character.
        ch: char,
        /// Byte position in the input string.
        position: usize,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },
    /// Input length is not valid for base64.
    #[snafu(display("invalid base64 length: {length}"))]
    InvalidLength {
        /// The length of the input.
        length: usize,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },
    /// Padding is malformed or absent where required.
    #[snafu(display("invalid base64 padding"))]
    InvalidPadding {
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },
}
```

```rust
pub fn encode (input: &[u8]) -> String
```

> Decode standard base64 (with `+`, `/`, `=` padding).
> 
> # Errors
> 
> Returns [`DecodeError`] if the input contains invalid characters,
> has an invalid length, or has malformed padding.
```rust
pub fn decode (input: &str) -> Result<Vec<u8>, DecodeError>
```

```rust
pub fn encode_url_safe_no_pad (input: &[u8]) -> String
```

> Decode URL-safe base64 (with `-`, `_`, no padding required).
> 
> Leniently accepts `+` and `/` as aliases for `-` and `_`, and strips
> any trailing `=` padding, so callers that receive mildly malformed
> inputs do not fail unnecessarily.
> 
> # Errors
> 
> Returns [`DecodeError`] if the input contains invalid characters
> or has an invalid length.
```rust
pub fn decode_url_safe_no_pad (input: &str) -> Result<Vec<u8>, DecodeError>
```

## `src/cleanup.rs`

> Registry that collects multiple cleanup callbacks and runs them all on drop.
> 
> `CleanupRegistry` accumulates callbacks over time and runs them in reverse
> registration order (LIFO) on drop -- matching the natural resource
> acquisition/release pattern.
> 
> # Example
> 
> ```
> use koina::cleanup::CleanupRegistry;
> use std::sync::Arc;
> use std::sync::atomic::{AtomicU32, Ordering};
> 
> let counter = Arc::new(AtomicU32::new(0));
> let mut registry = CleanupRegistry::new();
> 
> let c = Arc::clone(&counter);
> registry.register(move || { c.fetch_add(1, Ordering::Relaxed); });
> 
> let c = Arc::clone(&counter);
> registry.register(move || { c.fetch_add(10, Ordering::Relaxed); });
> 
> drop(registry);
> assert_eq!(counter.load(Ordering::Relaxed), 11, "both callbacks must fire");
> ```
```rust
pub struct CleanupRegistry {
    callbacks: Vec<Box<dyn FnOnce() + Send + Sync>>,
}
```

```rust
impl CleanupRegistry {
    pub fn new () -> Self;
    pub fn register (&mut self, callback: impl FnOnce() + Send + Sync + 'static);
    pub fn disarm (&mut self);
}
```

## `src/credential.rs`

```rust
pub enum CredentialSource {
    /// Read from an environment variable.
    Environment,
    /// Read from a credential file on disk.
    File,
    /// Obtained via OAuth token refresh.
    OAuth,
    /// Read from the OS keyring (e.g. GNOME Keyring, macOS Keychain).
    Keyring,
}
```

> A resolved credential paired with its source.
```rust
pub struct Credential {
    /// The secret value (API key or access token).
    pub secret: SecretString,
    /// Where this credential was obtained from.
    pub source: CredentialSource,
}
```

> Trait for credential resolution. Called per-request to support mid-session
> token rotation and background OAuth refresh.
> 
> Implementations must be `Send + Sync` for use across threads and in async
> contexts. The `get_credential()` method is intentionally synchronous: the
> refreshing providers store the current token in memory and refresh
> asynchronously in a background task.
```rust
pub trait CredentialProvider : Send + Sync {
    fn get_credential (&self) -> Option<Credential>;
    fn name (&self) -> &str;
}
```

## `src/defaults.rs`

> Default configuration file path relative to instance root.
```rust
pub const DEFAULT_CONFIG_PATH: &str = "config/aletheia.toml";
```

> Default LLM model identifier.
> 
> Single source of truth for the model every aletheia subsystem defaults to
> when no explicit model is configured: `aletheia init` scaffold, `add-nous`
> CLI default, runtime spawn fallback (`SONNET_MODEL`), pylon request
> fallback, `agent_io` export fallback, melete distillation default, taxis
> `ModelSpec` default, and theatron wizard model picker.
> 
> Defining the default in two places (formerly `DEFAULT_MODEL` and
> `DEFAULT_MODEL_SHORT`, #4235) routed `aletheia init` to one model and
> runtime spawn/distillation to a different one  -  a silent downgrade
> invisible at config time. Keep this as the only model default constant in
> the workspace; `crates/koina/tests/model_default_consistency.rs` walks the
> source tree and fails loudly if a second `DEFAULT_MODEL*` constant
> reappears.
```rust
pub const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
```

> Default nous-agent identifier created by `aletheia init -y` and assumed by
> CLI subcommands that take `--nous-id`. Single source of truth so that
> `init`'s scaffolded agent and `ingest`'s default flag value cannot drift
> (#4245).
```rust
pub const DEFAULT_AGENT_ID: &str = "pronoea";
```

> Default maximum output tokens per LLM response.
```rust
pub const MAX_OUTPUT_TOKENS: u32 = 16_384;
```

> Default maximum tokens for bootstrap context injection.
```rust
pub const BOOTSTRAP_MAX_TOKENS: u32 = 40_000;
```

> Default context window budget (tokens).
```rust
pub const CONTEXT_TOKENS: u32 = 200_000;
```

> Default context window budget for Opus models (1M token context window).
```rust
pub const OPUS_CONTEXT_TOKENS: u32 = 1_000_000;
```

> Default maximum consecutive tool use iterations per turn.
```rust
pub const MAX_TOOL_ITERATIONS: u32 = 200;
```

> Default maximum bytes per tool result before truncation.
```rust
pub const MAX_TOOL_RESULT_BYTES: u32 = 32_768;
```

> Default LLM call timeout in seconds.
```rust
pub const TIMEOUT_SECONDS: u32 = 300;
```

> Default history budget ratio (fraction of remaining context for conversation history).
```rust
pub const HISTORY_BUDGET_RATIO: f64 = 0.6;
```

> Default characters-per-token estimate for budget calculations.
```rust
pub const CHARS_PER_TOKEN: u32 = 4;
```

> Maximum output bytes returned by a single tool call.
```rust
pub const MAX_OUTPUT_BYTES: usize = 50 * 1024;
```

> Default limit for consecutive no-progress turns before the mistake brake fires.
```rust
pub const DEFAULT_CONSECUTIVE_MISTAKE_LIMIT: u32 = 5;
```

## `src/disk_space.rs`

> Default warning threshold: 1 GB.
```rust
pub const DEFAULT_WARNING_BYTES: u64 = 1024 * BYTES_PER_MB;
```

> Default critical threshold: 100 MB.
```rust
pub const DEFAULT_CRITICAL_BYTES: u64 = 100 * BYTES_PER_MB;
```

```rust
pub enum DiskStatus {
    /// Available space is above the warning threshold.
    Ok {
        /// Bytes available on the filesystem.
        available_bytes: u64,
    },
    /// Available space is below the warning threshold but above critical.
    Warning {
        /// Bytes available on the filesystem.
        available_bytes: u64,
    },
    /// Available space is below the critical threshold.
    Critical {
        /// Bytes available on the filesystem.
        available_bytes: u64,
    },
}
```

```rust
impl DiskStatus {
    pub fn available_bytes (self) -> u64;
}
```

> Query available disk space for the filesystem containing `path`.
> 
> # Errors
> 
> Returns an I/O error if the `statvfs` syscall fails (e.g. path does not
> exist or is not accessible).
```rust
pub fn available_space (path: &Path) -> std::io::Result<u64>
```

> Check disk space and classify against thresholds.
```rust
pub fn check_disk_space (
    path: &Path,
    warning_bytes: u64,
    critical_bytes: u64,
) -> std::io::Result<DiskStatus>
```

```rust
pub struct DiskSpaceMonitor {
    cached_available: Arc<AtomicU64>,
    warn_threshold: u64,
    critical_threshold: u64,
}
```

```rust
impl DiskSpaceMonitor {
    pub fn new (warning_bytes: u64, critical_bytes: u64) -> Self;
    pub fn refresh (&self, path: &Path) -> std::io::Result<DiskStatus>;
    pub fn status (&self) -> DiskStatus;
    pub fn allow_non_essential_write (&self) -> bool;
    pub fn warning_bytes (&self) -> u64;
    pub fn critical_bytes (&self) -> u64;
}
```

## `src/error.rs`

```rust
pub enum Error {
    /// Failed to read a file.
    #[snafu(display("failed to read {}", path.display()))]
    ReadFile {
        /// The path that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to write a file.
    #[snafu(display("failed to write {}", path.display()))]
    WriteFile {
        /// The path that could not be written.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to create a directory.
    #[snafu(display("failed to create directory {}", path.display()))]
    CreateDir {
        /// The directory path that could not be created.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to serialize to JSON.
    #[snafu(display("JSON serialization failed"))]
    JsonSerialize {
        /// The underlying serialization error.
        source: serde_json::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to deserialize from JSON.
    #[snafu(display("JSON deserialization failed"))]
    JsonDeserialize {
        /// The underlying deserialization error.
        source: serde_json::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// An identifier was invalid.
    #[snafu(display("invalid identifier: {message}"))]
    InvalidId {
        /// Description of why the identifier is invalid.
        message: String,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },
}
```

## `src/error_class.rs`

```rust
pub enum ErrorClass {
    /// Transient failure — safe to retry with backoff.
    ///
    /// Examples: network timeout, 429 rate limit, provider 5xx, temporary
    /// resource unavailability.
    Transient,

    /// Permanent failure — retrying will not help.
    ///
    /// Examples: invalid input, authentication failure, missing resource,
    /// database corruption, unsupported model.
    Permanent,

    /// Classification cannot be determined — escalate to operator.
    ///
    /// Used when the error source is opaque or the variant is not yet mapped.
    Unknown,
}
```

```rust
pub enum ErrorAction {
    /// Retry the operation with exponential backoff.
    ///
    /// `max_attempts` is the total number of attempts (including the first).
    /// `backoff_base_ms` is the initial delay before the first retry.
    Retry {
        /// Total number of attempts (1 = try once, no retries).
        max_attempts: u32,
        /// Base delay in milliseconds for exponential backoff.
        backoff_base_ms: u64,
    },

    /// Escalate to the operator or parent agent.
    ///
    /// The error is serious enough that automated retry would mask it.
    Escalate,

    /// Surface a human-readable message to the user.
    ///
    /// Used for errors the user can act on (e.g. budget exhausted, auth
    /// required).  `user_message` is safe to display directly.
    Surface {
        /// Human-readable message for the end user.
        user_message: String,
    },

    /// Log and discard — no further action required.
    ///
    /// Used for benign, fully-handled failures that do not need operator
    /// visibility (e.g. optional feature not available).
    Ignore,
}
```

> A classifiable error: knows its own class and the action the caller should take.
> 
> Implement this on each concrete error type that flows through the pipeline.
> The pipeline uses `class()` and `action()` to decide retry vs escalate vs
> surface, replacing per-site `match` arms on individual error variants.
> 
> # Example
> 
> ```
> use koina::error_class::{Classifiable, ErrorAction, ErrorClass};
> 
> struct MyError;
> 
> impl Classifiable for MyError {
>     fn class(&self) -> ErrorClass {
>         ErrorClass::Transient
>     }
> 
>     fn action(&self) -> ErrorAction {
>         ErrorAction::Retry {
>             max_attempts: 3,
>             backoff_base_ms: 500,
>         }
>     }
> }
> ```
```rust
pub trait Classifiable {
    fn class (&self) -> ErrorClass;
    fn action (&self) -> ErrorAction;
}
```

## `src/event.rs`

```rust
pub enum LogLevel {
    /// Trace-level detail.
    Trace,
    /// Debug-level detail.
    Debug,
    /// Informational.
    Info,
    /// Warning.
    Warn,
    /// Error.
    Error,
}
```

> A typed internal event that produces both a log line and metric labels.
> 
> Implementors define what to log and what metric labels to attach.
> The [`EventEmitter`] handles dispatching to both sinks.
```rust
pub trait InternalEvent : Send + Sync {
    fn event_name (&self) -> &'static str;
    fn log_level (&self) -> LogLevel;
    fn log_message (&self) -> String;
    fn metric_labels (&self) -> Vec<(&'static str, String)>; // default impl
    fn metric_value (&self) -> f64; // default impl
}
```

```rust
pub struct EventEmitter {
    /// Total events emitted (monotonic counter).
    counter: Arc<AtomicU64>,
    /// Optional metric sink for forwarding to Prometheus or other collectors.
    metric_sink: Arc<Option<Box<MetricSink>>>,
}
```

```rust
impl EventEmitter {
    pub fn new () -> Self;
    pub fn with_metric_sink (
        sink: impl Fn(&str, &[(&str, String)], f64) + Send + Sync + 'static,
    ) -> Self;
    pub fn emit (&self, event: &impl InternalEvent);
    pub fn event_count (&self) -> u64;
}
```

## `src/fjall.rs`

> A fjall database handle with a write-serialization mutex and optional temp
> directory for ephemeral (test) stores.
> 
> The `_temp_dir` field's `Drop` implementation deletes the temporary directory
> when the store is dropped. The leading underscore suppresses `dead_code`
> warnings since the field is only needed for its `Drop` side effect.
```rust
pub struct FjallDb {
    /// The underlying fjall database.
    pub db: SingleWriterTxDatabase,
    /// Shared write mutex.
    ///
    /// WHY: `SingleWriterTxDatabase` serialises writers internally, but many
    /// Aletheia stores expose `&self` write methods (matching historical legacy `SQLite` backends
    /// that use interior mutability). This mutex ensures only one logical write
    /// runs at a time, matching that serial contract.
    pub write_lock: sync::Mutex<()>,
    /// Kept alive to auto-delete the temp directory when the store is dropped.
    ///
    /// WHY: the leading underscore signals that the field is unused for its value
    /// but needed for its `Drop` side effect. Clippy flags this as
    /// `pub_underscore_fields`, but consumers destructure `FjallDb` and need
    /// access to transfer ownership of the temp directory into their own struct.
    #[expect(
        clippy::pub_underscore_fields,
        reason = "consumers destructure FjallDb and transfer ownership of the TempDir guard"
    )]
    pub _temp_dir: Option<tempfile::TempDir>,
}
```

```rust
impl FjallDb {
    pub fn open_existing (path: &Path) -> Result<Self, FjallOpenError>;
    pub fn open (path: &Path, partitions: &[&str]) -> Result<Self, FjallOpenError>;
    pub fn open_temp (partitions: &[&str]) -> Result<Self, FjallOpenError>;
}
```

```rust
pub enum FjallOpenError {
    /// Failed to create the store directory.
    CreateDir {
        /// The path that could not be created.
        path: std::path::PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// Failed to create a temporary directory.
    TempDir {
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// Failed to open the fjall database or a partition.
    Open(String),
}
```

> ISO 8601 timestamp string for "now" using jiff.
> 
> Shared across fjall-backed stores that need consistent timestamp formatting.
```rust
pub fn now_iso () -> String
```

## `src/fs.rs`

> Validate that `path` resolves within `root` after canonicalization.
> 
> Follows the security standard's path validation sequence:
> normalize -> check `allowed_roots` -> canonicalize -> re-check `allowed_roots`.
> 
> For paths that do not yet exist on disk, the parent directory is
> canonicalized and the final component is appended. This handles the
> common pattern of validating a file path before creating it.
> 
> # Errors
> 
> Returns [`std::io::Error`] if:
> - The path contains `..` components (pre-canonicalization check).
> - The canonicalized path does not start with the canonicalized root.
> - Canonicalization itself fails (e.g. root directory does not exist).
```rust
pub fn validate_within_root (path: &Path, root: &Path) -> std::io::Result<PathBuf>
```

> Write `content` to `path` atomically with 0600 permissions.
> 
> 1. Creates parent directories if needed.
> 2. Writes to a `.tmp` sibling with mode 0600.
> 3. Renames atomically to the target path.
> 
> The two-step write prevents other processes from reading a partially-written
> file and ensures the final file is never world-readable.
> 
> # Errors
> 
> Returns an I/O error if any step (dir creation, write, rename) fails.
```rust
pub fn write_restricted (path: &Path, content: &[u8]) -> std::io::Result<()>
```

## `src/http.rs`

> `application/json` content type.
```rust
pub const CONTENT_TYPE_JSON: &str = "application/json";
```

> `text/event-stream` content type for SSE responses.
```rust
pub const CONTENT_TYPE_EVENT_STREAM: &str = "text/event-stream";
```

> Bearer token prefix including the trailing space.
```rust
pub const BEARER_PREFIX: &str = "Bearer ";
```

> API v1 route prefix.
```rust
pub const API_V1: &str = "/api/v1";
```

> Health check endpoint path.
```rust
pub const API_HEALTH: &str = "/api/health";
```

> TLS-protected URL scheme, including the `://` separator.
```rust
pub const HTTPS_SCHEME_PREFIX: &str = "https://";
```

```rust
pub fn has_http_or_https_scheme (url: &str) -> bool
```

```rust
pub fn is_plaintext_loopback_url (url: &str) -> bool
```

## `src/id.rs`

```rust
pub struct NousId(String);
```

```rust
impl NousId {
    pub fn new (id: impl Into<String>) -> Result<Self, IdError>;
    pub fn as_str (&self) -> &str;
}
```

```rust
pub struct SessionId(Uuid);
```

```rust
impl SessionId {
    pub fn new () -> Self;
    pub fn parse (s: &str) -> Result<Self, IdError>;
}
```

```rust
pub struct TurnId(u64);
```

```rust
impl TurnId {
    pub const fn new (n: u64) -> Self;
    pub const fn as_u64 (self) -> u64;
    pub const fn next (self) -> Self;
}
```

```rust
pub struct ToolName(String);
```

```rust
impl ToolName {
    pub fn from_static (name: &'static str) -> Self;
    pub fn new (name: impl Into<String>) -> Result<Self, IdError>;
    pub fn as_str (&self) -> &str;
}
```

```rust
pub enum IdError {
    /// The identifier was empty.
    Empty {
        /// The identifier type name (e.g. "`NousId`").
        kind: &'static str,
    },
    /// The identifier exceeded the maximum length.
    TooLong {
        /// The identifier type name (e.g. "`NousId`").
        kind: &'static str,
        /// Maximum allowed length.
        max: usize,
        /// Actual length that was provided.
        actual: usize,
    },
    /// The identifier contained invalid characters or format.
    InvalidFormat {
        /// The identifier type name (e.g. "`NousId`").
        kind: &'static str,
        /// The value that failed validation.
        value: String,
        /// Description of why the format is invalid.
        reason: String,
    },
}
```

## `src/metrics.rs`

```rust
pub struct MetricsRegistry {
    inner: Arc<Mutex<Registry>>,
}
```

```rust
impl MetricsRegistry {
    pub fn new () -> Self;
    pub fn with_registry <F, R> (&self, f: F) -> R where
        F: FnOnce(&mut Registry) -> R,;
    pub fn encode (&self, buffer: &mut String) -> Result<(), std::fmt::Error>;
}
```

## `src/output_buffer.rs`

```rust
pub struct OutputBuffer<T> {
    /// Named output queues.
    outputs: HashMap<String, Vec<T>>,
}
```

```rust
impl <T: Clone> OutputBuffer<T> {
    pub fn new () -> Self;
    pub fn register_output (&mut self, name: &str);
    pub fn register_dead_letter (&mut self);
    pub fn push (&mut self, event: T, output: &str) -> bool;
    pub fn fan_out (&mut self, event: T, targets: &[&str]) -> usize;
    pub fn route (&mut self, event: T, router: impl FnOnce(&T) -> &str) -> bool;
    pub fn drain (&mut self, output: &str) -> Vec<T>;
    pub fn drain_dead_letter (&mut self) -> Vec<T>;
    pub fn peek (&self, output: &str) -> &[T];
    pub fn len (&self, output: &str) -> usize;
    pub fn is_empty (&self, output: &str) -> bool;
    pub fn total_events (&self) -> usize;
    pub fn output_names (&self) -> Vec<&str>;
    pub fn has_output (&self, name: &str) -> bool;
    pub fn clear (&mut self);
}
```

## `src/redacting_layer.rs`

> Tracing layer that redacts sensitive field values in structured JSON output.
> 
> Fields matching `redact_fields` have their values replaced with
> `[REDACTED]`. Fields matching `truncate_fields` are capped at
> `truncate_length` characters. All string values are scanned for API key
> patterns via [`redact_sensitive`].
```rust
pub struct RedactingLayer<W> {
    writer: Mutex<W>,
    redact_fields: HashSet<String>,
    truncate_fields: HashSet<String>,
    truncate_length: usize,
}
```

```rust
impl <W> RedactingLayer<W> {
    pub fn new (
        writer: W,
        redact_fields: impl IntoIterator<Item = String>,
        truncate_fields: impl IntoIterator<Item = String>,
        truncate_length: usize,
    ) -> Self;
}
```

## `src/retry.rs`

```rust
pub enum BackoffStrategy {
    /// Constant delay between every retry attempt.
    Constant {
        /// Delay applied before each retry.
        delay: Duration,
    },
    /// Fixed sequence of delays, used in order per attempt.
    ///
    /// If the attempt index exceeds the sequence length, the last delay is
    /// reused. An empty sequence yields zero delay.
    Fixed {
        /// Ordered delays for successive retry attempts.
        delays: Vec<Duration>,
    },
    /// Exponential backoff: `base * factor^attempt`, capped at `max_delay`.
    Exponential {
        /// Initial delay for the first retry (attempt 0).
        base: Duration,
        /// Multiplier applied per attempt.
        factor: u32,
        /// Upper bound on the computed delay.
        max_delay: Duration,
    },
    /// Exponential backoff with ±25% random jitter to prevent thundering herd.
    ExponentialJitter {
        /// Initial delay for the first retry (attempt 0).
        base: Duration,
        /// Multiplier applied per attempt.
        factor: u32,
        /// Upper bound on the computed delay (before jitter).
        max_delay: Duration,
    },
}
```

```rust
impl BackoffStrategy {
    pub fn delay_for_attempt (&self, attempt: u32) -> Duration;
}
```

```rust
pub struct RetryConfig {
    /// Maximum number of retry attempts (not counting the initial attempt).
    pub max_retries: u32,
    /// Strategy for computing delays between retries.
    pub strategy: BackoffStrategy,
}
```

```rust
impl RetryConfig {
    pub async fn retry_async <F, Fut, T, E> (
        &self,
        mut operation: F,
        should_retry: impl Fn(&E) -> bool,
    ) -> Result<T, E> where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,;
    pub async fn retry_classified_async <F, Fut, T, E> (&self, mut operation: F) -> Result<T, E> where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: Classifiable,;
}
```

```rust
pub fn exponential_steps (attempt: u32, factor: u32, cap: u32) -> u32
```

## `src/secret.rs`

> A string holding a secret value (API key, token, password).
> 
> - `Debug` and `Display` print `[REDACTED]` instead of the value.
> - `Serialize` outputs `"[REDACTED]"` to prevent accidental logging via JSON.
> - `Deserialize` accepts the actual string value normally.
> - The backing memory is zeroed on drop via [`zeroize`].
> - Use [`.expose_secret()`](Self::expose_secret) for intentional access.
```rust
pub struct SecretString {
    inner: String,
}
```

```rust
impl SecretString {
    pub fn expose_secret (&self) -> &str;
}
```

## `src/system.rs`

> Abstraction over filesystem read, write, and query operations.
> 
> Implement this trait to substitute an in-memory store in tests instead of
> touching the real disk.
> 
> All directory-creation methods use `create_dir_all` semantics (they create
> the full ancestor chain as needed).
```rust
pub trait FileSystem : Send + Sync {
    fn read_file (&self, path: &Path) -> io::Result<Vec<u8>>;
    fn write_file (&self, path: &Path, contents: &[u8]) -> io::Result<()>;
    fn exists (&self, path: &Path) -> bool;
    fn is_file (&self, path: &Path) -> bool;
    fn create_dir (&self, path: &Path) -> io::Result<()>;
    fn list_dir (&self, path: &Path) -> io::Result<Vec<PathBuf>>;
    fn remove_file (&self, path: &Path) -> io::Result<()>;
    fn rename (&self, from: &Path, to: &Path) -> io::Result<()>;
}
```

> Abstraction over the system clock.
> 
> Use [`RealSystem`] in production and a frozen [`TestSystem`] in tests to
> obtain deterministic timestamps without sleeping.
```rust
pub trait Clock : Send + Sync {
    fn now (&self) -> Timestamp;
    fn elapsed (&self, since: Timestamp) -> SignedDuration;
}
```

> Abstraction over the process environment.
> 
> Use [`RealSystem`] in production and a pre-populated [`TestSystem`] in
> tests to avoid polluting or reading the real process environment.
```rust
pub trait Environment : Send + Sync {
    fn var (&self, name: &str) -> Option<String>;
    fn var_os (&self, name: &str) -> Option<OsString>;
    fn vars (&self) -> Vec<(String, String)>;
    fn current_dir (&self) -> io::Result<PathBuf>;
    fn temp_dir (&self) -> PathBuf;
    fn current_exe (&self) -> io::Result<PathBuf>;
    fn args (&self) -> Vec<String>;
}
```

```rust
pub struct RealSystem;
```

```rust
pub struct TestSystem {
    /// Byte contents of virtual files, keyed by absolute path.
    pub(crate) files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
    /// Set of virtual directories (populated automatically on write/add).
    pub(crate) dirs: Arc<Mutex<HashSet<PathBuf>>>,
    /// Frozen clock value.
    pub(crate) clock: Timestamp,
    /// Fake environment variables.
    pub(crate) env: HashMap<String, String>,
    /// Fake temporary directory returned by [`Environment::temp_dir`].
    pub(crate) temp_dir: PathBuf,
    /// Fake executable path returned by [`Environment::current_exe`].
    pub(crate) current_exe: PathBuf,
    /// Fake process arguments returned by [`Environment::args`].
    pub(crate) args: Vec<String>,
}
```

```rust
impl TestSystem {
    pub fn new () -> Self;
    pub fn with_clock (mut self, ts: Timestamp) -> Self;
    pub fn with_env (mut self, key: impl Into<String>, value: impl Into<String>) -> Self;
    pub fn with_temp_dir (mut self, path: impl Into<PathBuf>) -> Self;
    pub fn with_current_exe (mut self, path: impl Into<PathBuf>) -> Self;
    pub fn with_args (mut self, args: Vec<String>) -> Self;
    pub fn add_file (&mut self, path: impl Into<PathBuf>, contents: impl Into<Vec<u8>>);
    pub fn add_env (&mut self, key: impl Into<String>, value: impl Into<String>);
    pub fn file_paths (&self) -> Vec<PathBuf>;
    pub fn get_file (&self, path: &Path) -> Option<Vec<u8>>;
}
```

## `src/ulid.rs`

```rust
pub struct Ulid(u128);
```

```rust
impl Ulid {
    pub fn new () -> Self;
    pub const fn from_u128 (value: u128) -> Self;
    pub const fn as_u128 (self) -> u128;
    pub const fn timestamp_ms (self) -> u64;
}
```

```rust
pub struct DecodeError {
    reason: &'static str,
}
```

## `src/uuid.rs`

```rust
pub struct Uuid([u8; 16]);
```

```rust
impl Uuid {
    pub fn new_v4 () -> Self;
    pub fn from_u128 (v: u128) -> Self;
    pub fn parse_str (s: &str) -> Result<Self, UuidParseError>;
    pub fn as_bytes (&self) -> &[u8; 16];
    pub fn from_bytes (bytes: [u8; 16]) -> Self;
    pub fn as_u128 (&self) -> u128;
    pub fn is_nil (&self) -> bool;
    pub fn as_fields (&self) -> (u32, u16, u16, &[u8; 8]);
    pub fn from_fields (time_low: u32, time_mid: u16, time_hi: u16, rest: &[u8; 8]) -> Self;
    pub fn new_v1 (timestamp_100ns: u64, clock_seq: u16, node: &[u8; 6]) -> Self;
    pub fn get_timestamp (&self) -> Option<V1Timestamp>;
}
```

```rust
pub struct UuidParseError;
```

```rust
pub struct V1Timestamp(u64);
```

```rust
impl V1Timestamp {
    pub fn to_unix (&self) -> (u64, u32);
}
```

```rust
pub fn uuid_v4 () -> String
```
