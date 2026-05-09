# L3 API Index: hermeneus

Crate path: `crates/hermeneus`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/anthropic/batch.rs`

```rust
pub struct BatchRequest {
    pub requests: Vec<BatchItem>,
}
```

```rust
pub struct BatchItem {
    pub custom_id: String,
    /// Pre-serialized request body (`WireRequest` borrows and can't be stored).
    pub params: serde_json::Value,
}
```

```rust
pub struct BatchResponse {
    pub id: String,
    pub processing_status: String,
    pub request_counts: BatchRequestCounts,
    pub results_url: Option<String>,
}
```

```rust
pub struct BatchRequestCounts {
    pub processing: u32,
    pub succeeded: u32,
    pub errored: u32,
    pub canceled: u32,
    pub expired: u32,
}
```

```rust
pub struct BatchResult {
    pub custom_id: String,
    pub result: BatchResultType,
}
```

```rust
pub enum BatchResultType {
    #[serde(rename = "succeeded")]
    Succeeded { message: serde_json::Value },
    #[serde(rename = "errored")]
    Errored { error: BatchError },
}
```

```rust
pub struct BatchError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}
```

## `src/anthropic/cc_profile.rs`

```rust
impl CcProfile {
    pub fn from_installed_cli () -> Self;
    pub fn with_context_1m (&mut self);
    pub fn attribution_header (&self, _first_message_text: &str) -> String;
    pub fn user_agent (&self) -> String;
    pub fn beta_header_value (&self) -> String;
}
```

## `src/anthropic/client.rs`

> Runtime-configurable provider behavior overrides.
>
> Passed to [`AnthropicProvider::with_credential_provider_and_behavior`] to
> parameterize constants that were previously hardcoded. Values come from
> [`taxis::config::ProviderBehaviorConfig`].
```rust
pub struct ProviderBehavior {
    /// Per-request timeout for non-streaming completions.
    pub non_streaming_timeout: Duration,
    /// Default retry delay in milliseconds for SSE stream errors.
    pub sse_retry_ms: u64,
}
```

> Anthropic Messages API provider.
```rust
pub struct AnthropicProvider {
    // kanon:ignore RUST/pub-visibility
    client: Client,
    credential_provider: Arc<dyn CredentialProvider>,
    base_url: String,
    api_version: String,
    max_retries: u32,
    pricing: HashMap<String, ModelPricing>,
    health: Arc<ProviderHealthTracker>,
    /// CC profile for request mimicry. `Some` when using OAuth credentials.
    cc_profile: Option<super::cc_profile::CcProfile>,
    /// Per-request timeout for non-streaming completions.
    non_streaming_timeout: Duration,
    /// Prompt cache policy (#3410). When `Disabled`, all `cache_control`
    /// markers are scrubbed before the wire request is built so operator
    /// content never enters Anthropic's prompt cache.
    prompt_cache_mode: PromptCacheMode,
}
```

```rust
impl AnthropicProvider {
    pub fn from_config (config: &ProviderConfig) -> Result<Self>;
    pub fn with_credential_provider (
        // kanon:ignore RUST/pub-visibility
        provider: Arc<dyn CredentialProvider>,
        config: &ProviderConfig,
    ) -> Result<Self>;
    pub fn with_credential_provider_and_behavior (
        // kanon:ignore RUST/pub-visibility
        provider: Arc<dyn CredentialProvider>,
        config: &ProviderConfig,
        behavior: &ProviderBehavior,
    ) -> Result<Self>;
    pub async fn complete_streaming (
        &self,
        request: &CompletionRequest,
        mut on_event: impl FnMut(StreamEvent) + Send,
    ) -> Result<CompletionResponse>;
}
```

## `src/anthropic/stream/mod.rs`

```rust
pub enum StreamEvent {
    /// Incremental text content.
    TextDelta { text: String },
    /// Incremental thinking content.
    ThinkingDelta { thinking: String },
    /// Incremental tool input JSON.
    InputJsonDelta { partial_json: String },
    /// A content block has started.
    ContentBlockStart {
        /// Zero-based position in the response content array.
        index: u32,
        /// Block type: `"text"`, `"tool_use"`, or `"thinking"`.
        block_type: String,
    },
    /// A content block has finished.
    ContentBlockStop {
        /// Zero-based position of the completed block.
        index: u32,
    },
    /// Message started with initial usage.
    MessageStart {
        /// Input token counts reported at message start.
        usage: Usage,
    },
    /// Message finished with final stop reason and usage.
    MessageStop {
        /// Why the model stopped generating.
        stop_reason: StopReason,
        /// Final cumulative token usage for the entire message.
        usage: Usage,
    },
}
```

## `src/cc/provider.rs`

```rust
pub struct CcProviderConfig {
    /// Path to the `claude` binary. If `None`, resolved from `PATH`.
    pub cc_binary: Option<PathBuf>,
    /// Default model when the request doesn't specify one.
    pub default_model: String,
    /// Subprocess timeout (wall-clock).
    pub timeout: Duration,
}
```

> Claude Code subprocess LLM provider.
>
> Delegates completions to the `claude` CLI binary via `-p --output-format stream-json`.
> CC manages its own authentication (OAuth token refresh, attestation headers)
> so the provider only needs to spawn the process and parse output.
```rust
pub struct CcProvider {
    // kanon:ignore RUST/pub-visibility
    cc_binary: PathBuf,
    default_model: String,
    timeout: Duration,
}
```

```rust
impl CcProvider {
    pub fn new (config: &CcProviderConfig) -> Result<Self>;
}
```

## `src/circuit_breaker.rs`

```rust
pub enum CircuitState {
    /// Accepting requests normally; counting consecutive failures.
    Closed,
    /// Rejecting all requests; waiting for cooldown before allowing a probe.
    Open {
        /// When the circuit opened.
        since: Timestamp,
    },
    /// Allowing a single probe request to test provider recovery.
    HalfOpen,
}
```

```rust
pub struct CircuitBreakerConfig {
    /// Consecutive failures required to open the circuit. Default: 5.
    pub failure_threshold: u32,
    /// Base cooldown before transitioning `Open` → `HalfOpen` (ms). Default: `30_000`.
    pub open_duration_ms: u64,
    /// Multiplier applied to `open_duration_ms` after each failed probe. Default: 2.0.
    pub backoff_multiplier: f64,
    /// Maximum backoff duration (ms). Default: `300_000` (5 minutes).
    pub backoff_max_ms: u64,
}
```

> Circuit breaker for a single LLM provider.
>
> Thread-safe via `std::sync::Mutex`: no lock is held across `.await`.
>
> # State machine
>
> ```text
> Closed ──[threshold failures]──▶ Open
>   ▲                               │
>   │         [cooldown elapsed]    │
>   │    ┌──── HalfOpen ◀───────────┘
>   └────┤
>        │    [probe fails]
>        └──▶ Open (backoff *= multiplier)
> ```
```rust
pub struct CircuitBreaker {
    // kanon:ignore RUST/pub-visibility
    inner: Mutex<CircuitBreakerInner>,
    config: CircuitBreakerConfig,
    provider_name: String,
}
```

```rust
impl CircuitBreaker {
    pub fn new (provider_name: impl Into<String>, config: CircuitBreakerConfig) -> Self;
    pub fn with_defaults (provider_name: impl Into<String>) -> Self;
    pub fn state (&self) -> CircuitState;
    pub fn is_allowed (&self) -> bool;
    pub fn on_success (&self);
    pub fn on_failure (&self);
}
```

## `src/complexity/mod.rs`

```rust
pub enum ModelTier {
    /// Fast, cheap, sufficient for simple queries.
    Haiku,
    /// Balanced capability and cost.
    Sonnet,
    /// Maximum capability for hard problems.
    Opus,
}
```

```rust
pub struct ComplexityInput<'a> {
    /// The user's message text.
    pub message_text: &'a str,
    /// Number of tools available in the current context.
    pub tool_count: usize,
    /// Number of messages already in the conversation.
    pub message_count: usize,
    /// Nesting depth for cross-agent calls (0 = top-level).
    pub depth: u32,
    /// Agent-level override from configuration (bypasses scoring).
    pub tier_override: Option<ModelTier>,
    /// Explicit model override from the user (bypasses routing entirely).
    pub model_override: Option<&'a str>,
}
```

```rust
pub struct ComplexityConfig {
    /// Whether complexity-based routing is enabled.
    pub enabled: bool,
    /// Score at or below which queries route to `haiku_model`.
    pub low_threshold: u32,
    /// Score at or above which queries route to `opus_model`.
    pub high_threshold: u32,
    /// Model identifier for the fast/cheap tier.
    pub haiku_model: String,
    /// Model identifier for the balanced tier.
    pub sonnet_model: String,
    /// Model identifier for the high-capability tier.
    pub opus_model: String,
}
```

```rust
pub struct ComplexityScore {
    /// Numeric score (0--100).
    pub score: u32,
    /// Determined model tier.
    pub tier: ModelTier,
    /// Human-readable factors that contributed to the score.
    pub reason: String,
}
```

```rust
pub struct RoutingDecision {
    /// Selected model identifier.
    pub model: String,
    /// Complexity score that drove the decision.
    pub complexity: ComplexityScore,
    /// Whether the user explicitly overrode model selection.
    pub is_override: bool,
}
```

```rust
pub struct RoutingOutcome {
    /// The routing decision that was made.
    pub decision: RoutingDecision,
    /// Whether the response was successful.
    pub success: bool,
    /// Whether the model self-escalated (indicated it needed more capability).
    pub self_escalated: bool,
}
```

```rust
pub fn score_complexity (input: &ComplexityInput<'_>) -> ComplexityScore
```

```rust
pub fn route_model (input: &ComplexityInput<'_>, config: &ComplexityConfig) -> RoutingDecision
```

## `src/concurrency.rs`

```rust
pub enum RequestOutcome {
    /// Request succeeded; increase the limit.
    Success,
    /// Request timed out or received 429; decrease the limit.
    Overload,
    /// Request was cancelled or outcome is unknown; no limit adjustment.
    Neutral,
}
```

```rust
pub struct ConcurrencyConfig {
    /// Starting concurrency limit. Default: 10.
    pub initial_limit: u32,
    /// Minimum concurrency limit (floor). Default: 1.
    pub min_limit: u32,
    /// Maximum concurrency limit (ceiling). Default: 200.
    pub max_limit: u32,
    /// Additive increase step on success. Default: 1.
    pub increase_step: u32,
    /// Multiplicative decrease factor on overload (must be in `(0.0, 1.0)`). Default: 0.9.
    pub decrease_factor: f64,
    /// EWMA smoothing factor for latency estimation (`0.0..1.0`).
    /// Higher values weight history more heavily. Default: 0.8.
    pub ewma_alpha: f64,
    /// Latency threshold in seconds. When the EWMA latency exceeds this value,
    /// new successes are treated as overload (triggering multiplicative decrease).
    /// Default: 30.0.
    pub latency_threshold_secs: f64,
}
```

> AIMD adaptive concurrency limiter for LLM calls with latency-based back-pressure.
>
> Callers acquire a [`ConcurrencyPermit`] before sending a request.
> On permit release the outcome and latency are applied, adjusting the limit.
>
> When the EWMA latency exceeds [`ConcurrencyConfig::latency_threshold_secs`],
> successes are treated as overload and the limit decreases multiplicatively.
> When latency drops below the threshold, additive increase resumes.
>
> Thread-safe; `acquire` is async and parks the caller when at capacity.
>
> # Example
>
> ```rust,no_run
> # use hermeneus::concurrency::{AdaptiveConcurrencyLimiter, ConcurrencyConfig, RequestOutcome};
> # use std::sync::Arc;
> # use std::time::Duration;
> # async fn example() {
> let limiter = Arc::new(AdaptiveConcurrencyLimiter::new("anthropic", ConcurrencyConfig::default()));
> let permit = limiter.acquire().await;
> // ... call the provider ...
> permit.finish_with_latency(RequestOutcome::Success, Duration::from_secs(2));
> # }
> ```
```rust
pub struct AdaptiveConcurrencyLimiter {
    // kanon:ignore RUST/pub-visibility
    inner: Mutex<LimiterInner>,
    notify: Notify,
    config: ConcurrencyConfig,
    provider_name: String,
}
```

```rust
impl AdaptiveConcurrencyLimiter {
    pub fn new (provider_name: impl Into<String>, config: ConcurrencyConfig) -> Self;
    pub fn with_defaults (provider_name: impl Into<String>) -> Self;
    pub fn limit (&self) -> u32;
    pub fn in_flight (&self) -> u32;
    pub fn latency_ewma (&self) -> Option<f64>;
    pub async fn acquire (self: &Arc<Self>) -> ConcurrencyPermit;
}
```

> RAII permit that holds a concurrency slot.
>
> Call [`finish`](ConcurrencyPermit::finish) or
> [`finish_with_latency`](ConcurrencyPermit::finish_with_latency) to record
> the outcome explicitly. If dropped without calling either, a `Neutral`
> outcome is applied with the elapsed time as latency.
```rust
pub struct ConcurrencyPermit {
    // kanon:ignore RUST/pub-visibility
    limiter: Arc<AdaptiveConcurrencyLimiter>,
    /// Encoded outcome; written by `finish`, read by `Drop`.
    outcome: AtomicU8,
    /// Set to 1 once released so `Drop` does not double-release.
    released: AtomicU8,
    /// When the permit was acquired, used for automatic latency measurement.
    start: Instant,
}
```

```rust
impl ConcurrencyPermit {
    pub fn finish (self, outcome: RequestOutcome);
    pub fn finish_with_latency (self, outcome: RequestOutcome, latency: Duration);
}
```

```rust
pub struct ConcurrencyLayer {
    limiter: Arc<AdaptiveConcurrencyLimiter>,
}
```

```rust
impl ConcurrencyLayer {
    pub fn new (limiter: Arc<AdaptiveConcurrencyLimiter>) -> Self;
}
```

```rust
pub struct ConcurrencyService<S> {
    inner: S,
    limiter: Arc<AdaptiveConcurrencyLimiter>,
}
```

```rust
impl <S> ConcurrencyService<S> {
    pub fn limiter (&self) -> &Arc<AdaptiveConcurrencyLimiter>;
}
```

## `src/error.rs`

```rust
pub struct ApiErrorContext {
    /// Model requested when the error occurred.
    pub model: String,
    /// Credential source used (e.g. `"oauth"`, `"environment"`, `"file"`).
    pub credential_source: String, // kanon:ignore RUST/plain-string-secret
}
```

```rust
impl ApiErrorContext {
    pub fn empty () -> Box<Self>;
}
```

```rust
pub enum Error {
    // kanon:ignore RUST/pub-visibility
    /// Provider initialization failed.
    #[snafu(display("provider init failed: {message}"))]
    ProviderInit {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// API request failed.
    #[snafu(display("API request failed: {message}"))]
    ApiRequest {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// API returned an error response.
    #[snafu(display("API error {status}: {message}"))]
    ApiError {
        status: u16,
        message: String,
        /// Diagnostic context (model + credential source).
        ///
        /// Boxed so that the variant stays within clippy's `result_large_err`
        /// limit. `hermeneus::Error` is embedded as a `source` field inside
        /// `nous::Error`, and two unboxed `String` fields would push the
        /// `nous::Error` variant size over 128 bytes.
        context: Box<ApiErrorContext>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Rate limited (429).
    #[snafu(display("rate limited, retry after {retry_after_ms}ms"))]
    RateLimited {
        retry_after_ms: u64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Response parsing failed.
    #[snafu(display("failed to parse response: {source}"))]
    ParseResponse {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Model not supported by this provider.
    #[snafu(display("model not supported: {model}"))]
    UnsupportedModel {
        model: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Authentication failed.
    #[snafu(display("authentication failed: {message}"))]
    AuthFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

```rust
impl Error {
    pub fn is_retryable (&self) -> bool;
}
```

> Convenience alias for `Result<T, Error>`.
```rust
pub type Result<T> = std::result::Result<T, Error>;
```

## `src/fallback.rs`

```rust
pub struct FallbackConfig {
    /// Ordered fallback models to try after the primary fails.
    pub fallback_models: Vec<String>,
    /// How many times to call the provider for each model before moving
    /// to the next. Each call uses the provider's internal retry logic.
    pub retries_before_fallback: u32,
}
```

```rust
pub async fn complete_with_fallback (
    provider: &dyn LlmProvider,
    request: &CompletionRequest,
    config: &FallbackConfig,
) -> Result<CompletionResponse>
```

## `src/health.rs`

```rust
pub enum ProviderHealth {
    /// Provider is responding normally.
    Up,
    /// Provider has had recent errors but is still accepting requests.
    Degraded {
        /// Number of errors since the last successful request.
        consecutive_errors: u32,
        /// When the most recent error occurred.
        last_error_at: Timestamp,
    },
    /// Provider is unavailable.
    Down {
        /// When the provider entered the Down state.
        since: Timestamp,
        /// What caused the transition to Down.
        reason: DownReason,
    },
}
```

```rust
pub enum DownReason {
    /// Too many consecutive failures.
    ConsecutiveFailures,
    /// Provider returned 429 with retry-after.
    RateLimited {
        /// Milliseconds to wait before retrying, from the `retry-after` header.
        retry_after_ms: u64,
    },
    /// Authentication failed: no auto-recovery.
    AuthFailure,
    /// Request timed out repeatedly.
    Timeout,
}
```

```rust
pub struct HealthConfig {
    /// Consecutive errors before Degraded → Down. Default: 5.
    pub consecutive_failure_threshold: u32,
    /// Cooldown before retrying a Down provider (ms). Default: `60_000`.
    pub down_cooldown_ms: u64,
}
```

> Tracks health for a single LLM provider.
>
> Thread-safe via `std::sync::Mutex`: all operations are short
> (no `.await` while holding the lock).
```rust
pub struct ProviderHealthTracker {
    // kanon:ignore RUST/pub-visibility
    inner: Mutex<TrackerInner>,
    config: HealthConfig,
}
```

```rust
impl ProviderHealthTracker {
    pub fn new (config: HealthConfig) -> Self;
    pub fn health (&self) -> ProviderHealth;
    pub fn check_available (&self) -> Result<(), ProviderHealth>;
    pub fn record_success (&self);
    pub fn record_error (&self, error: &Error);
}
```

## `src/loop_detector/mod.rs`

```rust
pub struct ToolCallSignature {
    /// `SipHash` of the tool name.
    pub name_hash: u64,
    /// `SipHash` of the serialized arguments.
    pub args_hash: u64,
    /// `SipHash` of the serialized result.
    pub result_hash: u64,
}
```

```rust
impl ToolCallSignature {
    pub fn from_parts (name: &str, args: &str, result: &str) -> Self;
}
```

```rust
pub enum LoopDetectorError {
    /// `k` consecutive identical tool calls were observed.
    #[snafu(display("doom loop detected: {k} consecutive identical calls for tool {tool}"))]
    DoomLoopDetected {
        /// Display name of the tool that looped.
        tool: String,
        /// Threshold that triggered.
        k: usize,
    },

    /// A-B-A-B-A oscillation between two distinct tool calls was observed.
    #[snafu(display(
        "ping-pong detected: alternating between {tool_a} and {tool_b} \
         ({k} signatures)"
    ))]
    PingPongDetected {
        /// Display identifier of the first tool.
        tool_a: String,
        /// Display identifier of the second tool.
        tool_b: String,
        /// Window size that triggered.
        k: usize,
    },

    /// `limit` consecutive turns produced the same assistant output without
    /// advancing state.
    #[snafu(display(
        "no progress detected: {consecutive} consecutive turns with identical \
         assistant output (limit {limit})"
    ))]
    NoProgressDetected {
        /// Current consecutive count.
        consecutive: u32,
        /// Threshold that triggered.
        limit: u32,
    },
}
```

```rust
pub struct DoomLoopDetector {
    ring: Vec<ToolCallSignature>,
    capacity: usize,
    k: usize,
}
```

```rust
impl DoomLoopDetector {
    pub fn new (capacity: usize, k: usize) -> Self;
    pub fn record (&mut self, sig: ToolCallSignature) -> Result<(), LoopDetectorError>;
    pub fn reset (&mut self);
}
```

```rust
pub struct PingPongDetector {
    ring: VecDeque<ToolCallSignature>,
    capacity: usize,
    k: usize,
}
```

```rust
impl PingPongDetector {
    pub fn new (capacity: usize, k: usize) -> Self;
    pub fn record (&mut self, sig: ToolCallSignature) -> Result<(), LoopDetectorError>;
    pub fn reset (&mut self);
}
```

```rust
pub struct NoProgressDetector {
    last_assistant_hash: Option<u64>,
    consecutive_no_progress: u32,
    limit: u32,
}
```

```rust
impl NoProgressDetector {
    pub fn new (limit: u32) -> Self;
    pub fn record_turn (
        &mut self,
        assistant_hash: u64,
        tool_called: bool,
    ) -> Result<(), LoopDetectorError>;
    pub fn reset (&mut self);
}
```

```rust
pub struct LoopGuard {
    doom: DoomLoopDetector,
    ping_pong: PingPongDetector,
    no_progress: NoProgressDetector,
}
```

```rust
impl LoopGuard {
    pub fn new () -> Self;
    pub fn with_limits (doom_k: usize, ping_pong_k: usize, no_progress_limit: u32) -> Self;
    pub fn record (
        &mut self,
        content: &str,
        reasoning: &str,
        tool_calls: &[(&str, &str, &str)],
    ) -> Result<(), LoopDetectorError>;
    pub fn reset_on_user_message (&mut self);
}
```

## `src/metrics.rs`

> Register this crate's metrics with the shared registry.
>
> Called once at startup. Counter names registered here drop the `_total`
> suffix because `prometheus-client` appends it automatically during
> exposition  -  register `aletheia_llm_tokens`, not `aletheia_llm_tokens_total`.
```rust
pub fn register (registry: &mut Registry)
```

> Record a completed LLM API call.
```rust
pub fn record_completion (
    provider: &str,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: f64,
    success: bool,
)
```

> Record LLM request latency.
```rust
pub fn record_latency (model: &str, status: &str, duration_secs: f64)
```

> Record time to first token (streaming only).
```rust
pub fn record_ttft (model: &str, status: &str, duration_secs: f64)
```

> Record cache token usage from a completed LLM API call.
```rust
pub fn record_cache_tokens (provider: &str, cache_read_tokens: u64, cache_write_tokens: u64)
```

## `src/models.rs`

> Default Anthropic API base URL.
```rust
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
```

> Default Anthropic API version header value.
```rust
pub const DEFAULT_API_VERSION: &str = "2023-06-01";
```

> Default maximum retry attempts for transient failures.
```rust
pub const DEFAULT_MAX_RETRIES: u32 = 3;
```

> Retry backoff base delay in milliseconds.
```rust
pub const BACKOFF_BASE_MS: u64 = 1000;
```

> Retry backoff multiplier per attempt.
```rust
pub const BACKOFF_FACTOR: u64 = 2;
```

> Maximum retry backoff delay in milliseconds.
```rust
pub const BACKOFF_MAX_MS: u64 = 30_000;
```

> All supported Anthropic model identifiers.
>
> Includes both short names (e.g., `claude-opus-4-6`) and dated snapshots
> (e.g., `claude-opus-4-20250514`).
```rust
pub static SUPPORTED_MODELS: &[&str] = &[
    // kanon:ignore RUST/pub-visibility
    "claude-opus-4-6",
    "claude-opus-4-20250514",
    "claude-sonnet-4-6",
    "claude-sonnet-4-20250514",
    "claude-haiku-4-5",
    "claude-haiku-4-5-20251001",
];
```

> Default Opus model alias.
```rust
pub const OPUS: &str = "claude-opus-4-6";
```

> Default Sonnet model alias.
```rust
pub const SONNET: &str = "claude-sonnet-4-6";
```

> Default Haiku model alias.
```rust
pub const HAIKU: &str = "claude-haiku-4-5-20251001";
```

## `src/openai/client.rs`

```rust
pub struct OpenAiProviderConfig {
    /// Operator-facing label used for logs, metrics, and `name()`.
    pub name: String,
    /// Base URL for the target endpoint — typically ends in `/v1`. Example:
    /// `http://127.0.0.1:8088/v1` for a local llama.cpp server. TLS is
    /// required unless the URL is loopback.
    pub base_url: String,
    /// Optional bearer token for authenticated endpoints. Loopback llama.cpp
    /// and ollama accept any value (or no auth at all); OpenAI requires a
    /// real key.
    pub api_key: Option<SecretString>,
    /// Model IDs this provider advertises support for. Determines routing
    /// in the [`ProviderRegistry`](crate::provider::ProviderRegistry).
    pub models: Vec<String>,
    /// Per-request timeout override. Defaults to 2 minutes (matches
    /// Anthropic's non-streaming default).
    pub request_timeout: Duration,
    /// Maximum retries on transient failures (5xx, timeout, connection
    /// reset). Defaults to 3.
    pub max_retries: u32,
    /// Where this provider's traffic terminates, gating which
    /// [`FactSensitivity`](mneme::knowledge::FactSensitivity) the recall
    /// pipeline is allowed to send to it (#3736, #3404, #3413).
    ///
    /// Defaults to [`DeploymentTarget::Cloud`] — the safe assumption that
    /// matches the trait default so existing TOML configurations without an
    /// explicit `deployment_target` key keep their Cloud-classified
    /// behaviour. Operators running a loopback llama.cpp, logismos, or
    /// ollama endpoint MUST set this to `local_hosted` or `embedded` in
    /// `aletheia.toml` so the recall filter lets `Internal` /
    /// `Confidential` facts through to the non-cloud boundary.
    pub deployment_target: DeploymentTarget,
}
```

> OpenAI Chat Completions-compatible LLM provider.
```rust
pub struct OpenAiProvider {
    client: Client,
    config: OpenAiProviderConfig,
    /// Owned `&'static str` slice of model IDs for [`LlmProvider::supported_models`].
    /// Leaked once at construction — the provider lives for the server lifetime.
    model_refs: &'static [&'static str],
    health: Arc<ProviderHealthTracker>,
}
```

```rust
impl OpenAiProvider {
    pub fn new (config: OpenAiProviderConfig) -> Result<Self>;
}
```

## `src/provider.rs`

> Trait for LLM providers.
>
> Implementations handle authentication, request formatting, response parsing,
> and error mapping. The provider translates between the generic types in
> [`types`](crate::types) and the wire format of the specific API.
>
> `Send + Sync` required for use in async contexts and across threads.
> Async methods return boxed futures to preserve `dyn LlmProvider` compatibility.
```rust
pub trait LlmProvider : Send + Sync {
    fn complete <'a> (
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>>;
    fn supported_models (&self) -> &[&str];
    fn supports_model (&self, model: &str) -> bool; // default impl
    fn name (&self) -> &str;
    fn deployment_target (&self) -> DeploymentTarget; // default impl
    fn supports_streaming (&self) -> bool; // default impl
    fn complete_streaming <'a> (
        &'a self,
        request: &'a CompletionRequest,
        _on_event: &'a mut (dyn FnMut(StreamEvent) + Send),
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>>; // default impl
}
```

```rust
pub struct ModelPricing {
    /// Cost per million input tokens (USD).
    pub input_cost_per_mtok: f64,
    /// Cost per million output tokens (USD).
    pub output_cost_per_mtok: f64,
}
```

```rust
pub enum PromptCacheMode {
    /// No `cache_control` markers emitted. Operator content never enters
    /// Anthropic's prompt cache. Default for sovereignty-first deployments.
    #[default]
    Disabled,
    /// Standard 5-minute ephemeral cache. `cache_control: {"type": "ephemeral"}`
    /// on system prompt, tools, and recent conversation turns.
    Ephemeral,
    /// Extended 1-hour cache. Currently behaves like [`Ephemeral`](Self::Ephemeral)
    /// since the wire format for extended TTL is provider-specific and not
    /// yet wired through. Reserved for future use.
    Extended,
}
```

```rust
pub enum DeploymentTarget {
    /// External cloud provider; receives only `Public` facts.
    #[default]
    Cloud,
    /// Self-hosted or network-local provider; receives `Public` and `Internal`.
    LocalHosted,
    /// In-process provider; no facts leave the host.
    Embedded,
}
```

```rust
impl DeploymentTarget {
    pub fn as_str (self) -> &'static str;
}
```

```rust
pub struct ProviderConfig {
    /// Provider type: `anthropic`, `openai`, `ollama`.
    pub provider_type: String,
    /// API key or credential reference.
    pub api_key: Option<SecretString>,
    /// Base URL override (for proxies or self-hosted).
    pub base_url: Option<String>,
    /// Default model to use.
    pub default_model: Option<String>,
    /// Maximum retries on transient failures.
    pub max_retries: Option<u32>,
    /// Per-model pricing for cost metrics. Keyed by model name.
    #[serde(default)]
    pub pricing: HashMap<String, ModelPricing>,
    /// Enable CC request mimicry for OAuth credentials. Defaults to `true`
    /// when using `with_credential_provider` against the first-party API.
    /// Set to `false` to disable (e.g., when enforcement is lifted or
    /// using API keys).
    #[serde(default)]
    pub cc_mimicry: Option<bool>,
    /// Prompt cache policy. Defaults to [`PromptCacheMode::Disabled`] —
    /// no `cache_control` markers are emitted and operator content never
    /// enters Anthropic's cache infrastructure (#3410).
    #[serde(default)]
    pub prompt_cache_mode: PromptCacheMode,
    /// Where this provider runs, gating which `FactSensitivity` the recall
    /// pipeline is allowed to send to it (#3404, #3413). Defaults to
    /// [`DeploymentTarget::Cloud`] — the safe assumption that an
    /// unconfigured provider speaks to an external service.
    #[serde(default)]
    pub deployment_target: DeploymentTarget,
}
```

```rust
pub struct ProviderRegistry {
    // kanon:ignore RUST/pub-visibility
    providers: Vec<ProviderEntry>,
}
```

```rust
impl ProviderRegistry {
    pub fn new () -> Self;
    pub fn register (&mut self, provider: Box<dyn LlmProvider>);
    pub fn register_with_config (&mut self, provider: Box<dyn LlmProvider>, config: HealthConfig);
    pub fn find_provider (&self, model: &str) -> Option<&dyn LlmProvider>;
    pub fn providers (&self) -> Vec<&dyn LlmProvider>;
    pub fn provider_health (&self, name: &str) -> Option<ProviderHealth>;
    pub fn record_success (&self, name: &str);
    pub fn find_streaming_provider (&self, model: &str) -> Option<&dyn LlmProvider>;
    pub fn record_error (&self, name: &str, error: &error::Error);
}
```

## `src/secret.rs`

```rust
pub enum SecretError {
    /// The requested secret is not present in the vault.
    #[snafu(display("secret `{name}` not in session store"))]
    MissingSecret {
        /// Name of the missing secret.
        name: String,
    },
}
```

```rust
pub struct SecretVault {
    inner: RwLock<HashMap<String, SecretString>>,
}
```

```rust
impl SecretVault {
    pub fn new () -> Self;
    pub fn store (&self, name: impl Into<String>, value: impl Into<SecretString>);
    pub fn get (&self, name: &str) -> Option<SecretString>;
    pub fn remove (&self, name: &str) -> Option<SecretString>;
    pub fn list_names (&self) -> Vec<String>;
    pub fn clear (&self);
    pub fn len (&self) -> usize;
    pub fn is_empty (&self) -> bool;
}
```

> Substitute `{{secret:<name>}}` and `$SECRET(<name>)` placeholders in a
> JSON value with the corresponding secret from `vault`.
>
> Substitution is recursive: it descends into objects and arrays.
> If a placeholder references a missing secret, returns [`SecretError::MissingSecret`].
>
> # Security note
>
> This mutates `value` in place. Callers should clone the original if the
> placeholder-bearing JSON is needed for persistence (e.g. transcript storage).
```rust
pub fn substitute_in_json (
    value: &mut serde_json::Value,
    vault: &SecretVault,
) -> Result<(), SecretError>
```

> Redact likely-secret string values inside a JSON value, replacing them
> with `"[REDACTED]"`.
>
> This is defense-in-depth: if a secret value leaks into a JSON payload
> (e.g. via a tool result), the redaction pass prevents it from flowing
> outward to logs or LLM providers.
>
> The heuristic is conservative: strings longer than 32 characters that
> contain no whitespace and are not already placeholders are treated as
> sensitive.
```rust
pub fn redact_in_json (value: &mut serde_json::Value)
```

## `src/test_utils.rs`

```rust
pub fn make_response (text: &str) -> CompletionResponse
```

> Configurable mock LLM provider for tests.
>
> Supports fixed responses, response queues, request capturing,
> and error injection. Thread-safe via `std::sync::Mutex` (lock never
> held across `.await`).
>
> # Examples
>
> Fixed text response:
> ```ignore
> let provider = MockProvider::new("Hello!");
> ```
>
> Response sequence (pops from front, repeats last):
> ```ignore
> let provider = MockProvider::with_responses(vec![r1, r2]);
> ```
>
> Error injection:
> ```ignore
> let provider = MockProvider::error("network timeout");
> ```
```rust
pub struct MockProvider {
    // kanon:ignore RUST/pub-visibility
    // WHY: std::sync::Mutex is intentional: lock never held across .await
    responses: Mutex<Vec<CompletionResponse>>,
    error: Mutex<Option<error::Error>>,
    models: &'static [&'static str],
    provider_name: &'static str,
    requests: Mutex<Vec<CompletionRequest>>,
}
```

```rust
impl MockProvider {
    pub fn new (text: &str) -> Self;
    pub fn with_responses (responses: Vec<CompletionResponse>) -> Self;
    pub fn error (message: &str) -> Self;
    pub fn models (mut self, models: &'static [&'static str]) -> Self;
    pub fn named (mut self, name: &'static str) -> Self;
    pub fn captured_requests (&self) -> Vec<CompletionRequest>;
}
```

## `src/types.rs`

```rust
pub enum ToolResultType {
    /// File read/edit/write operations (TTL: 5 minutes).
    FileOperation,
    /// Shell/bash command output (TTL: 3 minutes).
    ShellOutput,
    /// Search, grep, glob results (TTL: 2 minutes).
    SearchResult,
    /// Web search or fetch results (TTL: 2 minutes).
    WebResult,
    /// Unclassified tool output (no automatic TTL).
    Other,
}
```

```rust
impl ToolResultType {
    pub fn classify (tool_name: &str) -> Self;
}
```

```rust
pub struct ToolResultAge {
    /// When the tool result was created.
    pub created_at: jiff::Timestamp,
    /// Classified tool type for TTL lookup.
    pub tool_type: ToolResultType,
    /// Original token count before any compaction.
    pub original_tokens: u64,
}
```

```rust
pub struct Message {
    /// Message role.
    pub role: Role,
    /// Message content (text or structured blocks).
    pub content: Content,
    /// WHY(#3781): when true, this message is a cache breakpoint where
    /// the prefix up to and including this message should be cached
    /// via `cache_control: ephemeral`. Typically set on distilled
    /// summary messages so subsequent turns reuse the cached context.
    #[serde(default)]
    pub cache_breakpoint: bool,
}
```

```rust
pub enum Role {
    /// System prompt (Anthropic: separate field, `OpenAI`: system message).
    System,
    /// User message.
    User,
    /// Assistant response.
    Assistant,
}
```

```rust
impl Role {
    pub fn as_str (self) -> &'static str;
}
```

```rust
pub enum Content {
    /// Plain text content.
    Text(String),
    /// Structured content blocks (text, tool use, tool result, thinking).
    Blocks(Vec<ContentBlock>),
}
```

```rust
impl Content {
    pub fn text (&self) -> String;
}
```

```rust
pub enum ContentBlock {
    /// Text content.
    #[serde(rename = "text")]
    Text {
        /// The text content string.
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        /// Source citations attached to this text block.
        citations: Option<Vec<Citation>>,
    },

    /// Tool use request from assistant.
    #[serde(rename = "tool_use")]
    ToolUse {
        /// Provider-assigned tool use identifier (used to correlate with [`ToolResult`](ContentBlock::ToolResult)).
        id: String,
        /// Tool name matching a registered [`ToolDefinition::name`].
        name: String,
        /// Parsed JSON input arguments for the tool.
        input: serde_json::Value,
    },

    /// Tool result from user.
    #[serde(rename = "tool_result")]
    ToolResult {
        /// The [`ToolUse`](ContentBlock::ToolUse) `id` this result responds to.
        tool_use_id: String,
        /// Tool output content (text or rich content blocks).
        content: ToolResultContent,
        /// Whether the tool execution failed.
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },

    /// Extended thinking content.
    #[serde(rename = "thinking")]
    Thinking {
        /// The model's internal reasoning text.
        thinking: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        /// Cryptographic signature from the provider for encrypted thinking.
        signature: Option<String>,
    },

    /// Server-side tool use (informational, not dispatched locally).
    #[serde(rename = "server_tool_use")]
    ServerToolUse {
        /// Provider-assigned tool use identifier.
        id: String,
        /// Server tool name.
        name: String,
        /// Input arguments passed to the server tool.
        input: serde_json::Value,
    },

    /// Server-side web search tool result (opaque, round-tripped verbatim).
    #[serde(rename = "web_search_tool_result")]
    WebSearchToolResult {
        /// The tool use ID this result responds to.
        tool_use_id: String,
        /// Raw search result content returned by the server.
        content: serde_json::Value,
    },

    /// Server-side code execution result.
    ///
    /// Returned by the `code_execution_20250522` server tool. No client `tool_result`
    /// is needed: the server executed the code and returns stdout, stderr, and return code.
    #[serde(rename = "code_execution_result")]
    CodeExecutionResult {
        /// The Python code that was executed.
        code: String,
        /// Standard output from execution.
        stdout: String,
        /// Standard error from execution.
        stderr: String,
        /// Process return code (0 = success).
        return_code: i32,
    },
}
```

```rust
pub enum ToolResultContent {
    /// Simple text result (most common case, backward compatible).
    Text(String),
    /// Rich content blocks (text + images + documents).
    Blocks(Vec<ToolResultBlock>),
}
```

```rust
impl ToolResultContent {
    pub fn text (s: impl Into<String>) -> Self;
    pub fn blocks (blocks: Vec<ToolResultBlock>) -> Self;
    pub fn text_summary (&self) -> String;
}
```

```rust
pub enum ToolResultBlock {
    // kanon:ignore RUST/pub-visibility
    /// Text content.
    #[serde(rename = "text")]
    Text { text: String },
    /// Base64-encoded image.
    #[serde(rename = "image")]
    Image { source: ImageSource },
    /// Base64-encoded document (PDF).
    #[serde(rename = "document")]
    Document { source: DocumentSource },
}
```

```rust
pub struct ImageSource {
    /// Source type (always `"base64"`).
    #[serde(rename = "type")]
    pub source_type: String,
    /// MIME type (`"image/png"`, `"image/jpeg"`, `"image/gif"`, `"image/webp"`).
    pub media_type: String,
    /// Base64-encoded image data.
    pub data: String,
}
```

```rust
pub struct DocumentSource {
    /// Source type (always `"base64"`).
    #[serde(rename = "type")]
    pub source_type: String,
    /// MIME type (always `"application/pdf"`).
    pub media_type: String,
    /// Base64-encoded PDF data.
    pub data: String,
}
```

```rust
pub struct ServerToolDefinition {
    /// Server tool type identifier (e.g., `"web_search_20250305"`).
    #[serde(rename = "type")]
    pub tool_type: String,
    /// Display name.
    pub name: String,
    /// Maximum uses per turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<u32>,
    /// Allowed domains for web search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_domains: Option<Vec<String>>,
    /// Blocked domains for web search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_domains: Option<Vec<String>>,
    /// User location hint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_location: Option<serde_json::Value>,
}
```

```rust
pub struct ToolDefinition {
    /// Tool name (must match what the model calls).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the input parameters.
    pub input_schema: serde_json::Value,
    /// When true, the model returns `tool_use` blocks but does not execute them.
    /// The client must execute the tool and return a `tool_result`.
    /// This prevents the model from calling the tool via server-side passthrough.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_passthrough: Option<bool>,
}
```

```rust
impl CacheControl {
    pub fn ephemeral () -> Self;
}
```

```rust
pub enum ToolChoice {
    // kanon:ignore RUST/pub-visibility
    /// Let the model decide whether to use a tool.
    #[serde(rename = "auto")]
    Auto,
    /// Force the model to use at least one tool.
    #[serde(rename = "any")]
    Any,
    /// Force the model to use a specific tool by name.
    #[serde(rename = "tool")]
    Tool { name: String },
}
```

```rust
pub struct RequestMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    /// An opaque user identifier for provider-side tracking.
    pub user_id: Option<String>,
}
```

```rust
pub struct CitationConfig {
    /// Whether citation generation is enabled.
    pub enabled: bool,
}
```

```rust
pub enum Citation {
    // kanon:ignore RUST/pub-visibility
    /// Citation by character offset within a document.
    #[serde(rename = "char_location")]
    CharLocation {
        document_index: u32,
        start_char_index: u32,
        end_char_index: u32,
        cited_text: String,
    },
    /// Citation by page range within a document.
    #[serde(rename = "page_location")]
    PageLocation {
        document_index: u32,
        start_page: u32,
        end_page: u32,
        cited_text: String,
    },
    /// Citation from a web search result.
    #[serde(rename = "web_search_result_location")]
    WebSearchResultLocation {
        url: String,
        title: Option<String>,
        cited_text: String,
    },
}
```

```rust
pub struct CompletionRequest {
    // kanon:ignore RUST/struct-too-many-fields
    /// Model identifier (e.g. `claude-opus-4-20250514`).
    pub model: String,
    /// System prompt.
    pub system: Option<String>,
    /// Conversation messages.
    pub messages: Vec<Message>,
    /// Maximum output tokens.
    pub max_tokens: u32,
    /// Available user-defined tools.
    pub tools: Vec<ToolDefinition>,
    /// Server-side tools (e.g., web search) that execute on the provider's infrastructure.
    pub server_tools: Vec<ServerToolDefinition>,
    /// Temperature (0.0--1.0).
    pub temperature: Option<f32>,
    /// Whether to enable extended thinking.
    pub thinking: Option<ThinkingConfig>,
    /// Stop sequences.
    pub stop_sequences: Vec<String>,
    /// When true, system prompt gets `cache_control: ephemeral`.
    pub cache_system: bool,
    /// When true, last tool definition gets `cache_control: ephemeral`.
    pub cache_tools: bool,
    /// When true, recent non-current conversation turns get `cache_control: ephemeral`.
    pub cache_turns: bool,
    /// Control tool use behavior (auto/any/specific tool).
    pub tool_choice: Option<ToolChoice>,
    /// Request metadata for tracking.
    pub metadata: Option<RequestMetadata>,
    /// Enable citation tracking in responses.
    pub citations: Option<CitationConfig>,
}
```

```rust
pub struct ThinkingConfig {
    /// Whether thinking is enabled.
    pub enabled: bool,
    /// Maximum thinking tokens.
    pub budget_tokens: u32,
}
```

```rust
pub struct CompletionResponse {
    /// Response ID.
    pub id: String,
    /// Model used.
    pub model: String,
    /// Why the model stopped generating.
    pub stop_reason: StopReason,
    /// Response content blocks.
    pub content: Vec<ContentBlock>,
    /// Token usage.
    pub usage: Usage,
}
```

```rust
pub enum StopReason {
    /// Normal end of turn.
    EndTurn,
    /// Model wants to use a tool.
    ToolUse,
    /// Hit max tokens limit.
    MaxTokens,
    /// Hit a stop sequence.
    StopSequence,
}
```

```rust
impl StopReason {
    pub fn as_str (self) -> &'static str;
}
```

```rust
pub struct Usage {
    /// Input tokens consumed.
    pub input_tokens: u64,
    /// Output tokens generated.
    pub output_tokens: u64,
    /// Tokens read from cache.
    pub cache_read_tokens: u64,
    /// Tokens written to cache.
    pub cache_write_tokens: u64,
}
```

```rust
impl Usage {
    pub fn total (&self) -> u64;
}
```
