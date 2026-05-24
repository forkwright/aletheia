# L3 API Index: energeia

Crate path: `crates/energeia`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/agent_sdk.rs`

```rust
pub struct AgentSdkConfig {
    /// Default model identifier (e.g., "claude-opus-4", "claude-sonnet-4").
    pub default_model: String,
    /// Skip permission checks during dispatch.
    pub skip_permissions: bool,
    /// Disable MCP plugin loading.
    pub disable_plugins: bool,
    /// Optional `OAuth` token for API authentication.
    pub oauth_token: Option<String>,
    /// Optional MCP server configurations.
    pub mcp_servers: Vec<McpServerConfig>,
}
```

```rust
pub struct McpServerConfig {
    /// Server name/identifier.
    pub name: String,
    /// Server command to execute.
    pub command: String,
    /// Arguments for the server command.
    pub args: Vec<String>,
    /// Environment variables.
    pub env: HashMap<String, String>,
}
```

> Experimental Claude CLI dispatch engine.
> 
> WHY: Provides CLI subprocess integration with `OAuth` token injection,
> permissions, and MCP configuration fields while the native SDK path remains
> unwired. The public type name is kept for compatibility with existing
> configuration code, but the current transport is a `claude` CLI subprocess,
> not a native Agent SDK client.
```rust
pub struct AgentSdkEngine {
    config: AgentSdkConfig,
    binary: String,
}
```

```rust
impl AgentSdkEngine {
    pub fn new (config: AgentSdkConfig) -> Result<Self>;
    pub fn default_model (&self) -> &str;
    pub fn permissions_enabled (&self) -> bool;
    pub fn plugins_enabled (&self) -> bool;
}
```

## `src/backend/backend_impl.rs`

```rust
pub struct EnergeiaBackend {
    pub(crate) orchestrator: crate::orchestrator::Orchestrator,
    pub(crate) steward_config: crate::steward::StewardConfig,
    pub(crate) metrics: crate::metrics::MetricsService,
}
```

```rust
impl EnergeiaBackend {
    pub fn new (
        orchestrator: crate::orchestrator::Orchestrator,
        steward_config: crate::steward::StewardConfig,
        metrics: crate::metrics::MetricsService,
    ) -> Self;
    pub fn with_cancel_token (mut self, cancel: CancellationToken) -> Self;
}
```

## `src/backend.rs`

> High-level dispatch orchestration backend.
> 
> Abstracts the full dispatch workflow: execute prompts, manage PRs via
> steward, query status, check health, and generate reports. Control planes
> (kanon CLI, KAIROS daemon) depend on this trait, not on concrete
> implementations.
> 
> # Implementations
> 
> - [`EnergeiaBackend`]  -  uses energeia's [`Orchestrator`](crate::orchestrator::Orchestrator),
>   steward service, and fjall-backed metrics. This is the production backend
>   in aletheia. Requires the `storage-fjall` feature.
> - `PhronesisBackend` (in kanon)  -  wraps kanon's phronesis dispatch engine.
>   Exists for backwards compatibility during the migration period.
```rust
pub trait DispatchBackend : Send + Sync {
    fn dispatch <'a> (
        &'a self,
        spec: &'a DispatchSpec,
        prompts: &'a [PromptSpec],
    ) -> Pin<Box<dyn Future<Output = Result<DispatchResult>> + Send + 'a>>;
    fn steward_pass <'a> (
        &'a self,
        project: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<StewardResult>> + Send + 'a>>;
    fn status <'a> (&'a self) -> Pin<Box<dyn Future<Output = Result<StatusDashboard>> + Send + 'a>>;
    fn health <'a> (
        &'a self,
        window_days: u32,
    ) -> Pin<Box<dyn Future<Output = Result<HealthReport>> + Send + 'a>>;
    fn report <'a> (
        &'a self,
        days: u32,
    ) -> Pin<Box<dyn Future<Output = Result<CostReport>> + Send + 'a>>;
}
```

## `src/budget.rs`

```rust
pub enum BudgetStatus {
    /// All limits within bounds.
    Ok,
    /// Approaching a limit (informational, 80%+ consumed).
    Warning(String),
    /// A limit has been exceeded — sessions should be aborted.
    Exceeded(String),
}
```

> Shared budget tracker for a dispatch run.
> 
> INVARIANT: `current_cost_hundredths` and `current_turns` only increase
> (monotonic). `start_time` is set once at construction and never changes.
```rust
pub struct Budget {
    /// Maximum allowed aggregate cost in USD.
    pub max_cost_usd: Option<f64>,
    /// Maximum allowed aggregate agent turns.
    pub max_turns: Option<u32>,
    /// Maximum allowed wall-clock duration in milliseconds.
    pub max_duration_ms: Option<u64>,
    /// Accumulated cost in hundredths of a cent (1 USD = `10_000`) for atomic precision.
    ///
    /// WHY: Integer atomics avoid floating-point accumulation drift across many
    /// concurrent sessions. `10_000` hundredths per USD gives 0.01-cent precision.
    current_cost_hundredths: AtomicU64,
    current_turns: AtomicU32,
    start_time: Instant,
}
```

```rust
impl Budget {
    pub fn new (
        max_cost_usd: Option<f64>,
        max_turns: Option<u32>,
        max_duration_ms: Option<u64>,
    ) -> Self;
    pub fn record (&self, cost_usd: f64, turns: u32);
    pub fn check (&self) -> BudgetStatus;
    pub fn current_cost_usd (&self) -> f64;
    pub fn current_turns (&self) -> u32;
    pub fn elapsed_ms (&self) -> u64;
    pub fn cost_fraction (&self) -> f64;
    pub fn turn_fraction (&self) -> f64;
}
```

## `src/cost_ledger.rs`

```rust
pub struct BlastRadiusCost {
    /// Blast radius identifier (typically a file path or module prefix).
    pub blast_radius: String,
    /// Total cost in USD across all sessions targeting this blast radius.
    pub total_cost_usd: f64,
    /// Total LLM turns consumed across all sessions.
    pub total_turns: u32,
    /// Number of sessions recorded for this blast radius.
    pub session_count: u32,
    /// Cost breakdown by model used.
    pub cost_by_model: HashMap<String, f64>,
}
```

```rust
pub struct CostLedger {
    inner: Arc<Mutex<HashMap<String, BlastRadiusCost>>>,
}
```

```rust
impl CostLedger {
    pub fn new () -> Self;
    pub fn record (&self, blast_radius: &str, cost_usd: f64, turns: u32, model: &str);
    pub fn record_multi (&self, blast_radii: &[String], cost_usd: f64, turns: u32, model: &str);
    pub fn query (&self, blast_radius: &str) -> Option<BlastRadiusCost>;
    pub fn query_all (&self) -> Vec<(String, BlastRadiusCost)>;
    pub fn query_by_model (&self) -> Vec<(String, f64)>;
    pub fn total_cost (&self) -> f64;
    pub fn total_sessions (&self) -> u32;
    pub fn clear (&self);
}
```

## `src/cron.rs`

```rust
pub struct CronTask {
    /// Unique task identifier.
    pub name: CompactString,
    /// Cron schedule.
    pub cron: jiff_cron::Schedule,
    /// Maximum jitter to apply (+/- this duration).
    pub jitter: Duration,
    /// What to dispatch when this task fires.
    pub dispatch_spec: DispatchSpec,
}
```

```rust
impl CronTask {
    pub fn new (
        name: impl Into<CompactString>,
        schedule: &str,
        jitter: Duration,
        dispatch_spec: DispatchSpec,
    ) -> Result<Self>;
}
```

> Fjall-backed lock store that persists the last-fired timestamp per task.
> 
> A mutex serializes lock acquisition within a single process; the fjall
> write provides cross-restart deduplication.
```rust
pub struct CronLockStore {
    db: Arc<fjall::SingleWriterTxDatabase>,
    lock: parking_lot::Mutex<()>,
}
```

```rust
impl CronLockStore {
    pub fn open (db: Arc<fjall::SingleWriterTxDatabase>) -> Result<Self>;
    pub fn try_acquire (&self, task_name: &str, scheduled_time: Timestamp) -> Result<bool>;
    pub fn last_fired (&self, task_name: &str) -> Result<Option<Timestamp>>;
}
```

> Scheduler that manages a set of [`CronTask`]s with fjall-backed locking.
```rust
pub struct CronScheduler {
    tasks: Vec<CronTask>,
    lock_store: Arc<CronLockStore>,
}
```

```rust
impl CronScheduler {
    pub fn new (tasks: Vec<CronTask>, lock_store: Arc<CronLockStore>) -> Self;
    pub fn next_fire_after (&self, task: &CronTask, now: Zoned) -> Option<Zoned>;
    pub async fn run <F, Fut> (&self, cancel: CancellationToken, on_fire: F) -> Result<()> where
        F: Fn(CronTask) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,;
}
```

## `src/dag.rs`

```rust
pub enum PromptStatus {
    /// Node has been added to the graph but not yet evaluated.
    Pending,
    /// All dependencies satisfied — ready to dispatch.
    Ready,
    /// Currently being executed by an agent session.
    InProgress,
    /// Completed successfully.
    Done,
    /// Execution failed.
    Failed,
    /// Waiting on unsatisfied dependencies.
    Blocked,
}
```

```rust
pub struct DagNode {
    /// Unique prompt number.
    pub number: u32,
    /// Prompt numbers this prompt depends on (forward edges).
    pub depends_on: Vec<u32>,
    /// Current execution status.
    pub status: PromptStatus,
}
```

```rust
pub enum DagError {
    /// A cycle was detected in the dependency graph.
    #[snafu(display("cycle detected in dependency graph: {}", format_cycle(cycle)))]
    Cycle {
        /// Prompt numbers forming the cycle.
        cycle: Vec<u32>,
    },

    /// One or more prompts reference dependencies not present in the graph.
    #[snafu(display("{}", format_missing_deps(broken)))]
    MissingDependencies {
        /// All broken `(prompt, missing_dep)` pairs.
        broken: Vec<(u32, u32)>,
    },

    /// A prompt number was referenced but not found in the graph.
    #[snafu(display("prompt {number} not found in the graph"))]
    InvalidPrompt {
        /// The prompt number that was not found.
        number: u32,
    },

    /// Duplicate prompt number detected during construction.
    #[snafu(display("duplicate prompt number {number} in graph"))]
    DuplicateNode {
        /// The duplicate prompt number.
        number: u32,
    },
}
```

```rust
pub struct PromptDag {
    /// Prompt number to node mapping.
    pub(crate) nodes: HashMap<u32, DagNode>,
}
```

```rust
impl PromptDag {
    pub fn new () -> Self;
    pub fn add_node (&mut self, number: u32, depends_on: Vec<u32>) -> Result<(), DagError>;
    pub fn set_status (&mut self, number: u32, status: PromptStatus) -> Result<(), DagError>;
    pub fn get_ready (&self) -> Vec<u32>;
    pub fn validate (&self) -> Result<(), DagError>;
}
```

## `src/engine.rs`

```rust
pub trait DispatchEngine : Send + Sync {
    fn probe_health <'a> (
        &'a self,
        _timeout: Duration,
    ) -> Pin<Box<dyn Future<Output = Result<Option<u64>>> + Send + 'a>>; // default impl
    fn spawn_session <'a> (
        &'a self,
        spec: &'a SessionSpec,
        options: &'a AgentOptions,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>>;
    fn resume_session <'a> (
        &'a self,
        session_id: &'a str,
        prompt: &'a str,
        options: &'a AgentOptions,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>>;
}
```

> Handle to a running or completed agent session.
> 
> Provides an event stream for observing session progress, plus control
> methods for waiting on completion or aborting.
```rust
pub trait SessionHandle : Send {
    fn session_id (&self) -> &str;
    fn next_event <'a> (
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Option<SessionEvent>> + Send + 'a>>;
    fn wait (self: Box<Self>) -> Pin<Box<dyn Future<Output = Result<SessionResult>> + Send>>;
    fn abort <'a> (&'a mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
}
```

```rust
pub struct SessionSpec {
    /// The prompt or task description to send to the agent.
    pub prompt: String,
    /// System prompt to prepend to the session.
    pub system_prompt: Option<String>,
    /// Working directory for the agent session.
    pub cwd: Option<String>,
    /// Optional prompt cache components. When present, engines that support
    /// prompt caching should use `static_prefix` as the cached system prompt
    /// and `dynamic_suffix` as the user message.
    pub prompt_components: Option<crate::prompt_cache::PromptComponents>,
}
```

```rust
pub struct AgentOptions {
    /// LLM model identifier (e.g., "claude-sonnet-4-20250514").
    pub model: Option<String>,
    /// System prompt override.
    pub system_prompt: Option<String>,
    /// Working directory for the agent.
    pub cwd: Option<String>,
    /// Maximum LLM turns before the session is stopped.
    pub max_turns: Option<u32>,
    /// Permission mode for tool execution (e.g., "plan", "auto").
    pub permission_mode: Option<String>,
    /// Additional directories the agent can access.
    pub additional_dirs: Vec<PathBuf>,
}
```

```rust
impl AgentOptions {
    pub fn new () -> Self;
    pub fn model (mut self, model: impl Into<String>) -> Self;
    pub fn system_prompt (mut self, prompt: impl Into<String>) -> Self;
    pub fn cwd (mut self, cwd: impl Into<String>) -> Self;
    pub fn max_turns (mut self, turns: u32) -> Self;
    pub fn permission_mode (mut self, mode: impl Into<String>) -> Self;
    pub fn add_dir (mut self, dir: impl Into<PathBuf>) -> Self;
}
```

```rust
pub enum SessionEvent {
    /// Agent produced a text output chunk.
    TextDelta {
        /// The text content.
        text: String,
    },
    /// Agent invoked a tool.
    ToolUse {
        /// Tool name.
        name: String,
        /// Tool input as JSON.
        input: serde_json::Value,
    },
    /// Tool execution completed.
    ToolResult {
        /// Tool name.
        name: String,
        /// Whether the tool succeeded.
        success: bool,
    },
    /// Session turn completed.
    TurnComplete {
        /// Turn number within the session.
        turn: u32,
    },
    /// Session encountered an error.
    Error {
        /// Error description.
        message: String,
    },
}
```

```rust
pub struct SessionResult {
    /// The Agent SDK session identifier.
    pub session_id: String,
    /// Total cost in USD.
    pub cost_usd: f64,
    /// Total turns consumed.
    pub num_turns: u32,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Whether the session completed successfully.
    pub success: bool,
    /// Final text output from the agent, if any.
    pub result_text: Option<String>,
    /// LLM model used for this session, if known.
    pub model: Option<String>,
    /// Tokens read from the prompt cache.
    #[serde(default)]
    pub cache_hit_tokens: u64,
    /// Tokens written to the prompt cache.
    #[serde(default)]
    pub cache_miss_tokens: u64,
}
```

```rust
impl SessionResult {
    pub fn new (
        session_id: String,
        cost_usd: f64,
        num_turns: u32,
        duration_ms: u64,
        success: bool,
        result_text: Option<String>,
    ) -> Self;
}
```

## `src/error.rs`

```rust
pub enum Error {
    /// I/O error reading or listing prompt files.
    #[snafu(display("I/O error for '{}': {source}", path.display()))]
    Io {
        path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// YAML frontmatter parse error in a prompt file.
    #[snafu(display("frontmatter parse error in '{}': {detail}", path.display()))]
    FrontmatterParse {
        path: PathBuf,
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Cycle detected in the prompt dependency graph.
    #[snafu(display("cycle in prompt DAG: {}", cycle.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(" -> ")))]
    DagCycle {
        /// Prompt numbers forming the cycle.
        cycle: Vec<u32>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Broken or missing dependency references in the prompt DAG.
    #[snafu(display("broken prompt dependencies: {detail}"))]
    DagMissingDeps {
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Dispatch was aborted via cancellation.
    #[snafu(display("dispatch aborted"))]
    Aborted {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Budget limit exceeded during dispatch.
    #[snafu(display("budget exceeded: {reason}"))]
    BudgetExceeded {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session spawn failed for a specific prompt.
    #[snafu(display("spawn failed for prompt {prompt_number}: {detail}"))]
    SpawnFailed {
        prompt_number: u32,
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session resume failed.
    #[snafu(display("resume failed for session '{session_id}': {detail}"))]
    ResumeFailed {
        session_id: String,
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// QA evaluation failed.
    #[snafu(display("QA evaluation failed: {detail}"))]
    QaFailed {
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// LLM provider error during dispatch operations.
    #[snafu(display("LLM error: {detail}"))]
    Llm {
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Preflight validation failed before dispatch could start.
    #[snafu(display("preflight failed: {reason}"))]
    Preflight {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Blast radius overlap detected between concurrent sessions.
    #[snafu(display("blast radius overlap: {detail}"))]
    BlastRadiusOverlap {
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Task join error from a spawned session task.
    #[snafu(display("task join failed: {detail}"))]
    TaskJoin {
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Engine-level transport or protocol error.
    #[snafu(display("engine error: {detail}"))]
    Engine {
        detail: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// State store read/write failure.
    #[snafu(display("store error: {message}"))]
    Store {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Record serialization or deserialization failure.
    #[snafu(display("serialization error: {message}"))]
    Serialization {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Requested record not found.
    #[snafu(display("not found: {what}"))]
    NotFound {
        what: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Invalid cron expression.
    #[snafu(display("invalid cron expression '{expression}': {source}"))]
    CronParse {
        expression: String,
        source: jiff_cron::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Invalid model identifier specified.
    #[snafu(display("invalid model: {model}"))]
    InvalidModel {
        model: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

> Convenience alias for results with [`Error`].
```rust
pub type Result<T> = std::result::Result<T, Error>;
```

## `src/friction/capture.rs`

```rust
pub struct Observation {
    /// Human-readable title summarising the observation.
    pub title: String,
    /// Detailed description of the observation.
    pub body: String,
    /// URL of the PR where the observation was recorded.
    pub source: String,
    /// Files or crates affected by the observation.
    pub files_affected: Vec<String>,
}
```

> Template snippet for the "Observations" PR body section.
> 
> Prompt builders can inject this template so worker agents know
> how to format observations. Replace the `{{placeholder}}` tokens
> at render time.
```rust
pub const TEMPLATE: &str = r"## Observations

### {{title}}
{{body}}

- **Source:** {{source}}
- **Files affected:** {{files}}

";
```

```rust
pub fn parse_pr_body (text: &str) -> Vec<Observation>
```

## `src/frontier.rs`

```rust
pub fn compute_frontier (dag: &PromptDag) -> Vec<Vec<u32>>
```

## `src/hermeneus_engine.rs`

> Dispatch engine backed by hermeneus [`LlmProvider`].
> 
> Supports prompt caching via the [`PromptComponents`](crate::prompt_cache::PromptComponents)
> split: the static prefix is sent as a cached system prompt and the dynamic
> suffix as the user message.
```rust
pub struct HermeneusEngine {
    provider: Arc<dyn LlmProvider>,
    default_model: String,
    pricing: HashMap<String, ModelPricing>,
}
```

```rust
impl HermeneusEngine {
    pub fn new (provider: Arc<dyn LlmProvider>, default_model: impl Into<String>) -> Self;
    pub fn with_pricing (
        provider: Arc<dyn LlmProvider>,
        default_model: impl Into<String>,
        pricing: HashMap<String, ModelPricing>,
    ) -> Self;
}
```

## `src/http/client.rs`

> Subprocess-based dispatch engine targeting the Claude CLI.
> 
> Spawns `claude --output-format stream-json` subprocesses and streams NDJSON
> events. Will be replaced by a direct HTTP/SSE client when the Anthropic
> Agent SDK HTTP endpoints are publicly available.
```rust
pub struct HttpEngine {
    /// Default model identifier (e.g., "claude-sonnet-4-20250514").
    default_model: String,
    /// Path to the claude CLI binary.
    binary: String,
}
```

```rust
impl HttpEngine {
    pub fn new (default_model: impl Into<String>) -> Self;
}
```

## `src/http/mock.rs`

> Test double for [`DispatchEngine`].
> 
> Returns pre-configured outcomes in FIFO order. Thread-safe for use in
> async test contexts.
```rust
pub struct MockEngine {
    outcomes: Mutex<VecDeque<MockOutcome>>,
}
```

```rust
pub enum MockOutcome {
    /// Session completes successfully with the given events and result.
    Success {
        /// Events yielded by `next_event()` before the stream ends.
        events: Vec<SessionEvent>,
        /// Final result returned by `wait()`.
        result: SessionResult,
    },
    /// Session fails to spawn with the given error message.
    SpawnFailure {
        /// Error detail message.
        detail: String,
    },
}
```

```rust
impl MockEngine {
    pub fn new (outcomes: Vec<MockOutcome>) -> Self;
}
```

## `src/metrics/cost.rs`

```rust
pub struct CostReport {
    /// Start of the reporting window (inclusive).
    pub period_start: jiff::Timestamp,
    /// End of the reporting window (inclusive; the time the report was computed).
    pub period_end: jiff::Timestamp,
    /// Total cost across all dispatches in the window, in USD.
    pub total_cost_usd: f64,
    /// Total number of dispatches in the window.
    pub total_dispatches: u64,
    /// Total number of sessions across all dispatches in the window.
    pub total_sessions: u64,
    /// Average cost per dispatch (0.0 when `total_dispatches` is zero).
    pub avg_cost_per_dispatch: f64,
    /// Average cost per session (0.0 when `total_sessions` is zero).
    pub avg_cost_per_session: f64,
    /// Per-project cost breakdown, sorted by cost descending.
    pub by_project: Vec<ProjectCost>,
    /// Per-day velocity, sorted by date ascending.
    pub daily_velocity: Vec<DailyVelocity>,
}
```

```rust
pub struct ProjectCost {
    /// Project identifier.
    pub project: String,
    /// Total cost in USD.
    pub cost_usd: f64,
    /// Number of dispatches.
    pub dispatches: u64,
    /// Total sessions across dispatches.
    pub sessions: u64,
    /// Fraction of completed dispatches (0.0–1.0).
    pub success_rate: f64,
}
```

```rust
pub struct DailyVelocity {
    /// Calendar date (UTC).
    pub date: jiff::civil::Date,
    /// Number of dispatches created on this date.
    pub dispatches: u64,
    /// Total sessions across those dispatches.
    pub sessions: u64,
    /// Total cost in USD.
    pub cost_usd: f64,
    /// Fraction of dispatches that completed successfully (0.0–1.0).
    pub success_rate: f64,
}
```

> Compute a cost and velocity report for the given number of past days.
> 
> Pass `window_days = 0` to include all available history.
> Pass `window_days = 7` for the last week, `30` for the last month.
> 
> # Errors
> 
> Returns `Error::Store` if any underlying store read fails.
```rust
pub fn compute_cost_report (store: &EnergeiaStore, window_days: u32) -> Result<CostReport>
```

## `src/metrics/health.rs`

```rust
pub enum HealthStatus {
    /// Metric is within healthy bounds.
    Ok,
    /// Metric is degraded but not critical.
    Warn,
    /// Metric indicates a critical problem requiring attention.
    Crit,
    /// Insufficient data to compute the metric (sample size zero).
    Unavailable,
}
```

```rust
pub struct HealthMetric {
    /// Short `snake_case` identifier.
    pub name: &'static str,
    /// Human-readable description of what this metric measures.
    pub description: &'static str,
    /// Computed value: rate (0.0–1.0), hours, or count depending on metric.
    pub value: f64,
    /// Status classification of the current value.
    pub status: HealthStatus,
    /// Threshold at or beyond which the status is `Ok`.
    pub ok_threshold: f64,
    /// Threshold at or beyond which the status is `Warn` (between Ok and Crit).
    pub warn_threshold: f64,
    /// Number of samples used to compute this metric (0 means unavailable).
    pub sample_size: u64,
    /// `true` when the metric uses correlated proxy data instead of direct data
    /// for the named phenomenon.
    pub is_proxied: bool,
    /// `true` if a higher value is healthier (e.g. success rate).
    pub higher_is_better: bool,
    /// Engine name label for downstream metrics export.
    pub engine_name: &'static str,
    /// Provider label for downstream metrics export.
    pub provider: &'static str,
    /// Agent identifier label for downstream metrics export.
    pub agent_id: &'static str,
}
```

```rust
impl HealthMetric {
    pub fn is_available (&self) -> bool;
    pub fn uses_proxy_data (&self) -> bool;
}
```

```rust
pub struct HealthReport {
    /// When this report was computed.
    pub computed_at: jiff::Timestamp,
    /// Days of history included (0 = all available data).
    pub window_days: u32,
    /// All 7 pipeline health metrics.
    pub metrics: Vec<HealthMetric>,
}
```

```rust
impl HealthReport {
    pub fn proxy_metric_count (&self) -> usize;
}
```

> Compute all 7 pipeline health metrics from stored dispatch and session data.
> 
> `window_days` controls how far back to look; pass `0` to include all
> available data. All queries are read-only.
> 
> # Errors
> 
> Returns `Error::Store` if any underlying store read fails.
```rust
pub fn compute_health_report (store: &EnergeiaStore, window_days: u32) -> Result<HealthReport>
```

## `src/metrics/mod.rs`

```rust
pub struct MetricsService {
    store: Arc<EnergeiaStore>,
}
```

```rust
impl MetricsService {
    pub fn new (store: Arc<EnergeiaStore>) -> Self;
    pub fn health_report (&self, window_days: u32) -> Result<HealthReport>;
    pub fn cost_report (&self, window_days: u32) -> Result<CostReport>;
    pub fn status_dashboard (&self) -> Result<StatusDashboard>;
}
```

## `src/metrics/prometheus.rs`

> Register this crate's metrics with the shared registry.
```rust
pub fn register (registry: &mut Registry)
```

> Record a completed dispatch run.
> 
> Call once per dispatch when it finishes (Completed or Failed).
```rust
pub fn record_dispatch (project: &str, status: &str)
```

> Record a completed agent session.
> 
> - `cost_usd`  -  session cost; silently skipped when zero.
> - `duration_ms`  -  wall-clock duration in milliseconds.
> - `model`  -  LLM model used (e.g., "claude-3-5-sonnet").
> - `blast_radius`  -  blast radius identifier for cost attribution.
```rust
pub fn record_session (
    project: &str,
    status: &str,
    cost_usd: f64,
    duration_ms: u64,
    model: &str,
    blast_radius: &str,
)
```

> Record turns consumed by a session.
> 
> Call this to update the `energeia_turns_total` metric.
```rust
pub fn record_turns (project: &str, turns: u32, model: &str, blast_radius: &str)
```

> Record a QA evaluation verdict.
> 
> `verdict` should be one of `"pass"`, `"partial"`, or `"fail"`.
```rust
pub fn record_qa_verdict (project: &str, verdict: &str)
```

## `src/metrics/status.rs`

```rust
pub struct StatusDashboard {
    /// When this snapshot was taken.
    pub computed_at: jiff::Timestamp,
    /// Number of dispatches currently in `Running` state.
    pub active_dispatches: u64,
    /// Alias for `active_dispatches`; number of prompts awaiting completion.
    pub queue_depth: u64,
    /// Recent dispatch outcomes (newest first), up to `RECENT_LIMIT`.
    pub recent_outcomes: Vec<RecentOutcome>,
    /// Per-project summary aggregated from active and recent dispatches.
    pub by_project: Vec<ProjectSummary>,
}
```

```rust
pub struct RecentOutcome {
    /// Dispatch identifier.
    pub dispatch_id: String,
    /// Project this dispatch belongs to.
    pub project: String,
    /// Current lifecycle status.
    pub status: String,
    /// When the dispatch was created.
    pub started_at: jiff::Timestamp,
    /// When the dispatch finished (`None` if still running).
    pub finished_at: Option<jiff::Timestamp>,
    /// Number of sessions in this dispatch.
    pub total_sessions: u32,
    /// Total cost in USD across all sessions.
    pub total_cost_usd: f64,
}
```

```rust
pub struct ProjectSummary {
    /// Project identifier.
    pub project: String,
    /// Number of currently running dispatches.
    pub active_dispatches: u64,
    /// Total sessions across all dispatches in the recent window.
    pub total_sessions: u64,
    /// Total cost in USD across the recent window.
    pub total_cost_usd: f64,
    /// Fraction of completed dispatches in the recent window (0.0–1.0).
    pub success_rate: f64,
}
```

> Build a real-time status dashboard snapshot.
> 
> Scans all dispatches and sessions. The `recent_outcomes` list contains the
> most recent `50` dispatches sorted newest-first.
> 
> # Errors
> 
> Returns `Error::Store` if any underlying store read fails.
```rust
pub fn compute_status_dashboard (store: &EnergeiaStore) -> Result<StatusDashboard>
```

## `src/orchestrator/config.rs`

```rust
pub struct OrchestratorConfig {
    /// Maximum number of sessions executing concurrently within a group.
    /// Defaults to 4.
    pub max_concurrent: u32,
    /// Default cost budget in USD for the entire dispatch.
    /// `None` means no cost limit.
    pub default_budget_usd: Option<f64>,
    /// Default turn budget across all sessions in a dispatch.
    /// `None` means no turn limit.
    pub default_budget_turns: Option<u32>,
    /// Maximum wall-clock duration for the entire dispatch.
    /// `None` means no time limit.
    #[serde(with = "duration_ms_option")]
    pub max_duration: Option<Duration>,
    /// Idle timeout per session (no events within this window triggers timeout).
    /// `None` disables idle timeout detection.
    #[serde(with = "duration_ms_option")]
    pub session_idle_timeout: Option<Duration>,
    /// Maximum number of corrective prompt retries per failed prompt.
    /// Defaults to 0 (no corrective attempts unless explicitly configured).
    pub max_corrective_retries: u32,
    /// Optional role definition text or path to a role file.
    /// When present, the preparation stage splits prompts into a static
    /// prefix (role + standards + validation gate) and dynamic suffix.
    pub role: Option<String>,
    /// Optional directory containing standard `.md` files.
    pub standards_dir: Option<PathBuf>,
    /// List of standard names to include in the static prefix.
    pub standards: Vec<String>,
    /// Optional scope context appended to the dynamic suffix.
    pub scope: Option<String>,
    /// Additional directories the agent sessions may access.
    pub additional_dirs: Vec<PathBuf>,
}
```

```rust
impl OrchestratorConfig {
    pub fn new () -> Self;
    pub fn max_concurrent (mut self, n: u32) -> Self;
    pub fn default_budget_usd (mut self, usd: f64) -> Self;
    pub fn default_budget_turns (mut self, turns: u32) -> Self;
    pub fn max_duration (mut self, duration: Duration) -> Self;
    pub fn session_idle_timeout (mut self, timeout: Duration) -> Self;
    pub fn max_corrective_retries (mut self, n: u32) -> Self;
    pub fn role (mut self, role: impl Into<String>) -> Self;
    pub fn standards_dir (mut self, dir: impl Into<PathBuf>) -> Self;
    pub fn standards (mut self, standards: Vec<String>) -> Self;
    pub fn scope (mut self, scope: impl Into<String>) -> Self;
    pub fn add_dir (mut self, dir: impl Into<PathBuf>) -> Self;
}
```

## `src/orchestrator/mod.rs`

> Top-level dispatch orchestrator.
> 
> Builds the pipeline context, runs the 4-stage dispatch pipeline
> (preparation → execution → `post_processing`), and returns the result.
> Stage logic lives in [`crate::pipeline`]; this struct owns the
> engine/QA/store references and exposes the public API.
```rust
pub struct Orchestrator {
    engine: Arc<dyn DispatchEngine>,
    qa: Arc<dyn QaGate>,
    #[cfg(feature = "storage-fjall")]
    store: Option<Arc<crate::store::EnergeiaStore>>,
    config: OrchestratorConfig,
    cancel: CancellationToken,
}
```

```rust
impl Orchestrator {
    pub fn new (
        engine: Arc<dyn DispatchEngine>,
        qa: Arc<dyn QaGate>,
        config: OrchestratorConfig,
    ) -> Self;
    pub fn with_cancel_token (mut self, cancel: CancellationToken) -> Self;
    pub fn with_store (mut self, store: Arc<crate::store::EnergeiaStore>) -> Self;
    pub async fn dispatch (
        &self,
        spec: DispatchSpec,
        prompts: &[PromptSpec],
    ) -> Result<DispatchResult>;
    pub fn dry_run (&self, prompts: &[PromptSpec]) -> Result<DryRunResult>;
}
```

```rust
pub struct DryRunResult {
    /// Ordered groups of prompts that would execute together.
    pub groups: Vec<DryRunGroup>,
    /// Total number of prompts in the dispatch.
    pub total_prompts: usize,
    /// Maximum concurrency configured for the dispatch.
    pub max_concurrent: u32,
    /// Budget limit in USD (if configured).
    pub budget_usd: Option<f64>,
    /// Budget limit in turns (if configured).
    pub budget_turns: Option<u32>,
}
```

```rust
pub struct DryRunGroup {
    /// Zero-based group index (execution order).
    pub group_index: usize,
    /// Prompts in this group.
    pub prompts: Vec<DryRunPrompt>,
}
```

```rust
pub struct DryRunPrompt {
    /// Prompt number.
    pub number: u32,
    /// Task description.
    pub description: String,
    /// Dependencies (prompt numbers).
    pub depends_on: Vec<u32>,
}
```

## `src/predictive_budget.rs`

```rust
pub enum Complexity {
    /// Simple, mechanical changes (lint fixes, formatting, typos).
    Low,
    /// Moderate changes requiring some design (feature additions, tests).
    Medium,
    /// Architectural or large-scale redesign work.
    High,
}
```

```rust
pub struct ClassificationDetail {
    /// Assigned complexity tier.
    pub complexity: Complexity,
    /// Normalized score (0–10+) used for fine-grained adjustment.
    pub score: u32,
    /// Keywords or signals that influenced the classification.
    pub signals: Vec<String>,
}
```

```rust
pub struct PredictedBudget {
    /// Recommended turn budget for initial session run.
    initial_turns: u32,
    /// Recommended turn budget for each resume attempt.
    resume_turns: u32,
    /// Confidence level in the prediction (0.0–1.0).
    confidence: f64,
    /// Factors that contributed to the prediction.
    factors: PredictionFactors,
    /// Human-readable explanation of the prediction.
    explanation: String,
}
```

```rust
impl PredictedBudget {
    pub fn initial_turns (&self) -> u32;
    pub fn resume_turns (&self) -> u32;
    pub fn confidence (&self) -> f64;
    pub fn factors (&self) -> &PredictionFactors;
    pub fn explanation (&self) -> &str;
}
```

```rust
pub struct PredictionFactors {
    /// Complexity classification of the prompt.
    pub complexity: Complexity,
    /// Number of files in the blast radius.
    pub blast_radius_count: usize,
    /// Domain/type of the prompt (feat, fix, refactor, etc.).
    pub domain: String,
    /// Complexity score from classification.
    pub complexity_score: u32,
    /// Estimated based on historical data.
    pub historical_based: bool,
}
```

```rust
pub fn classify_with_detail (text: &str) -> ClassificationDetail
```

```rust
pub fn predict_budget (
    prompt: &PromptSpec,
    domain: Option<&str>,
    max_turns: Option<u32>,
) -> PredictedBudget
```

```rust
pub fn predict_budget_with_historical (
    prompt: &PromptSpec,
    domain: Option<&str>,
    max_turns: Option<u32>,
    historical_avg: Option<f64>,
) -> PredictedBudget
```

## `src/prompt.rs`

```rust
pub struct PromptSpec {
    /// Prompt number (unique within the project queue).
    pub number: u32,
    /// Human-readable description of the task.
    pub description: String,
    /// Prompt numbers this prompt depends on (DAG edges).
    pub depends_on: Vec<u32>,
    /// Acceptance criteria the implementation must satisfy.
    pub acceptance_criteria: Vec<String>,
    /// File paths the prompt is allowed to modify.
    pub blast_radius: Vec<String>,
    /// Full Markdown body (task instructions after the frontmatter delimiter).
    pub body: String,
    /// Optional prompt cache split. Populated by the preparation stage when
    /// role/standards configuration is present.
    #[serde(skip)]
    pub prompt_components: Option<crate::prompt_cache::PromptComponents>,
}
```

> Load a single prompt from a YAML-frontmatter Markdown file.
> 
> The file must begin with `---\n`, contain a YAML block, and close with
> `---\n`. Everything after the closing delimiter is the body.
> 
> # Errors
> 
> Returns [`crate::error::Error::Io`] on read failure or
> [`crate::error::Error::FrontmatterParse`] if the YAML is malformed or
> the file lacks the `---` delimiters.
```rust
pub fn load_prompt (path: &Path) -> Result<PromptSpec>
```

> Load all `.md` prompts from a directory.
> 
> Reads every `*.md` file in `dir` (non-recursive) and returns the parsed
> specs sorted by prompt number. Skips non-Markdown files silently.
> 
> # Errors
> 
> Returns [`crate::error::Error::Io`] if the directory cannot be read.
> Returns [`crate::error::Error::FrontmatterParse`] for any malformed file.
```rust
pub fn load_queue (dir: &Path) -> Result<Vec<PromptSpec>>
```

> Construct a validated [`PromptDag`] from a slice of prompt specs.
> 
> Each spec's `number` and `depends_on` fields form the DAG nodes and edges.
> Immediately validates the graph for cycles and missing dependencies.
> 
> # Errors
> 
> Returns [`crate::error::Error::DagCycle`] on cycle detection or
> [`crate::error::Error::DagMissingDeps`] for broken dependency references.
```rust
pub fn build_dag (prompts: &[PromptSpec]) -> Result<PromptDag>
```

## `src/prompt_cache.rs`

```rust
pub struct PromptComponents {
    /// Static content that can be prompt-cached: role definition + standards +
    /// validation gate. Identical across dispatches for the same role.
    pub static_prefix: String,
    /// Dynamic content that changes per dispatch: project state + scope +
    /// prompt body.
    pub dynamic_suffix: String,
}
```

```rust
impl PromptComponents {
    pub fn build (
        role: Option<&str>,
        project: &str,
        standards_dir: Option<&Path>,
        standards: &[String],
        scope: Option<&str>,
        prompt_body: &str,
    ) -> Self;
    pub fn to_full_prompt (&self) -> String;
    pub fn to_session_spec (&self, cwd: Option<String>) -> crate::engine::SessionSpec;
}
```

## `src/qa/corrective.rs`

```rust
pub fn generate_corrective (qa_result: &QaResult, original: &PromptSpec) -> Option<PromptSpec>
```

```rust
pub fn derive_failure_type (qa_result: &QaResult) -> String
```

## `src/qa/mechanical.rs`

```rust
pub fn mechanical_check (diff: &str, prompt: &PromptSpec) -> Vec<MechanicalIssue>
```

> Run `cargo fmt --check` in the given directory.
> 
> Returns a [`MechanicalIssue`] per file with formatting violations.
> Returns an empty vec on success or if the command cannot run.
> 
> # Cancel safety
> 
> Cancel-safe. The spawned `cargo fmt` process runs independently;
> cancelling this future simply detaches from the process. The process
> will complete and its output is discarded.
```rust
pub async fn format_check (working_dir: &Path) -> Vec<MechanicalIssue>
```

> Run `cargo clippy` in the given directory.
> 
> Returns a [`MechanicalIssue`] per warning or error detected.
> Returns an empty vec on success or if the command cannot run.
> 
> # Cancel safety
> 
> Cancel-safe. The spawned `cargo clippy` process runs independently;
> cancelling this future simply detaches from the process. The process
> will complete and its output is discarded.
```rust
pub async fn lint_check (working_dir: &Path) -> Vec<MechanicalIssue>
```

```rust
pub fn parse_changed_files (diff: &str) -> Vec<String>
```

## `src/qa/mod.rs`

> Abstraction over quality assurance evaluation.
> 
> Implementations use hermeneus for LLM-based semantic evaluation and
> perform mechanical checks (blast radius, lint, format) without LLM calls.
```rust
pub trait QaGate : Send + Sync {
    fn evaluate <'a> (
        &'a self,
        prompt: &'a PromptSpec,
        pr_number: u64,
        diff: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<QaResult>> + Send + 'a>>;
    fn mechanical_check (&self, diff: &str, prompt: &PromptSpec) -> Vec<MechanicalIssue>;
}
```

```rust
pub struct PromptSpec {
    /// Prompt number within the dispatch.
    pub prompt_number: u32,
    /// Human-readable task description.
    pub description: String,
    /// Acceptance criteria that the PR must satisfy.
    pub acceptance_criteria: Vec<String>,
    /// Files that the prompt is allowed to modify.
    pub blast_radius: Vec<String>,
}
```

```rust
impl PromptSpec {
    pub fn new (prompt_number: u32, description: String) -> Self;
}
```

> Run a full QA evaluation on a PR diff.
> 
> Orchestrates the complete flow: mechanical pre-screening, criteria
> classification, LLM evaluation of semantic criteria, verdict determination,
> and training data capture.
> 
> # Arguments
> 
> * `diff` - The unified diff of the PR
> * `prompt` - The prompt specification with criteria and blast radius
> * `pr_number` - The pull request number
> * `llm` - Optional LLM provider for semantic evaluation. When `None`,
>   semantic criteria are skipped with a warning and the verdict reflects
>   mechanical checks only.
> 
> # Semantic evaluation
> 
> When an LLM provider is supplied, semantic criteria are evaluated via
> hermeneus. When unavailable (`None`), the verdict indicates that
> semantic evaluation was not included so the operator knows the gate
> is mechanical-only.
```rust
pub async fn run_qa (
    diff: &str,
    prompt: &PromptSpec,
    pr_number: u64,
    llm: Option<&dyn LlmProvider>,
) -> QaResult
```

```rust
pub fn record_training_data (
    store: &crate::store::EnergeiaStore,
    qa_result: &QaResult,
    project: &str,
)
```

## `src/qa/semantic.rs`

```rust
pub fn classify_criteria (criteria: &[String]) -> Vec<(String, CriterionType)>
```

```rust
pub fn build_qa_prompt (
    description: &str,
    criteria: &[(String, CriterionType)],
    diff: &str,
) -> String
```

```rust
pub fn parse_qa_response (raw: &str, criteria: &[(String, CriterionType)]) -> Vec<CriterionResult>
```

## `src/qa/verdict.rs`

```rust
pub fn determine_verdict (
    criteria: &[CriterionResult],
    mechanical_issues: &[MechanicalIssue],
) -> QaVerdict
```

```rust
pub fn has_critical_mechanical_issues (issues: &[MechanicalIssue]) -> bool
```

## `src/resume.rs`

```rust
pub struct ResumePolicy {
    /// Ordered stages of escalating intervention.
    pub stages: Vec<ResumeStage>,
}
```

```rust
pub struct ResumeStage {
    /// Maximum turns allowed in this stage before escalating to the next.
    pub max_turns: u32,
    /// Message injected into the session at this escalation level.
    pub message: String,
}
```

```rust
impl ResumePolicy {
    pub fn next_stage (&self, current_turns: u32) -> Option<&ResumeStage>;
}
```

## `src/session/events.rs`

```rust
impl EventAccumulator {
    pub fn new () -> Self;
}
```

> Extract a `GitHub` pull request URL from text.
> 
> Matches `https://github.com/{owner}/{repo}/pull/{number}` patterns.
> Returns the first match found.
```rust
pub fn extract_pr_url (text: &str) -> Option<&str>
```

## `src/session/manager.rs`

```rust
impl SessionManager {
    pub fn new (
        engine: Arc<dyn DispatchEngine>,
        budget: Arc<Budget>,
        resume_policy: ResumePolicy,
    ) -> Self;
    pub async fn execute (
        &self,
        prompt: &PromptSpec,
        options: &EngineConfig,
    ) -> Result<SessionOutcome>;
}
```

## `src/session/options.rs`

```rust
pub struct EngineConfig {
    /// Base options passed to the [`DispatchEngine`](crate::engine::DispatchEngine).
    pub options: AgentOptions,
    /// Additional directories the agent can access.
    pub additional_dirs: Vec<PathBuf>,
    /// How long to wait for session events before declaring a timeout.
    /// `None` disables timeout detection.
    pub idle_timeout: Option<Duration>,
    /// Cancellation token shared by the dispatch group.
    pub cancel: Option<CancellationToken>,
}
```

```rust
impl EngineConfig {
    pub fn new (options: AgentOptions) -> Self;
    pub fn model (mut self, model: impl Into<String>) -> Self;
    pub fn system_prompt (mut self, prompt: impl Into<String>) -> Self;
    pub fn cwd (mut self, cwd: impl Into<PathBuf>) -> Self;
    pub fn max_turns (mut self, turns: u32) -> Self;
    pub fn permission_mode (mut self, mode: impl Into<String>) -> Self;
    pub fn add_dir (mut self, dir: impl Into<PathBuf>) -> Self;
    pub fn idle_timeout (mut self, timeout: Duration) -> Self;
    pub fn cancel_token (mut self, token: CancellationToken) -> Self;
    pub fn to_agent_options (&self) -> AgentOptions;
    pub fn options_with_turns (&self, turns: u32) -> AgentOptions;
}
```

## `src/steward/classify.rs`

```rust
pub fn determine_ci_status (checks: &[CheckRun], required_checks: &[String]) -> CiStatus
```

```rust
pub fn extract_prompt_number (pr: &PullRequest) -> Option<u32>
```

```rust
pub fn parse_suppressions (diff: &str) -> Vec<SuppressionFinding>
```

```rust
pub fn extract_qa_verdict_from_body (body: Option<&str>) -> Option<QaVerdictStatus>
```

```rust
pub fn apply_gate_trailer_override (
    ci_status: CiStatus,
    has_gate_trailer: bool,
    pr_number: u64,
) -> CiStatus
```

## `src/steward/conflict.rs`

```rust
pub fn build_rebase_prompt (pr_number: u64, branch_name: &str, repo_dir: &Path) -> String
```

## `src/steward/fix.rs`

```rust
pub fn classify_failure (check_name: &str, log_excerpt: &str) -> CiFailureKind
```

```rust
pub fn fix_kind_category (kind: &FixKind) -> CiFailureKind
```

## `src/steward/merge.rs`

```rust
pub fn make_merge_decision (
    classified: &ClassifiedPr,
    opts: &MergeOptions,
    diff: Option<&str>,
) -> MergeDecision
```

```rust
pub fn classify_merge_tier (classified: &ClassifiedPr, diff: Option<&str>) -> MergeTier
```

```rust
pub fn has_public_api_changes (diff: &str) -> bool
```

```rust
pub fn has_hold_flag (body: Option<&str>) -> bool
```

## `src/steward/overlap.rs`

```rust
pub fn compute_merge_order (prs: &[ClassifiedPr]) -> Vec<Vec<u64>>
```

```rust
pub fn file_overlap (a: &ClassifiedPr, b: &ClassifiedPr) -> Vec<String>
```

## `src/steward/service.rs`

```rust
pub struct StewardConfig {
    /// Polling interval between steward passes.
    pub interval: Duration,
    /// Whether to run a single pass and exit.
    pub once: bool,
    /// Dry-run mode: classify without executing actions.
    pub dry_run: bool,
    /// `GitHub` project slug (owner/repo).
    pub project: String,
    /// Required CI check names (empty = all checks matter).
    pub required_checks: Vec<String>,
}
```

```rust
impl StewardConfig {
    pub fn new (project: String) -> Self;
}
```

> Run the steward polling loop.
> 
> Each cycle: classify PRs, make merge decisions, execute actions.
> Respects the cancellation token for graceful shutdown.
> 
> WHY: Separating the polling loop from the single-pass logic allows
> both daemon mode (polling) and CLI mode (single pass).
> 
> # Cancel safety
> 
> Cancel-safe at loop boundaries. The `select!` uses `cancel.cancelled()`
> which is cancel-safe. Dropping the future between iterations simply
> delays the next poll without losing state.
```rust
pub async fn run (config: &StewardConfig, cancel: CancellationToken) -> Vec<StewardResult>
```

> Run a single steward pass (classify, decide, act).
> 
> This is the unit of work for both polling and single-pass modes.
> Returns the classification and action results.
> 
> # Cancel safety
> 
> Not cancel-safe. This is a placeholder implementation; the real
> implementation will perform side effects (fetching PRs, executing
> merges) that are not idempotent. Do not use in `select!` branches.
```rust
pub async fn run_once (config: &StewardConfig) -> StewardResult
```

## `src/steward/types.rs`

```rust
pub struct PullRequest {
    /// Pull request number (e.g., 42 for PR #42).
    pub number: u64,
    /// Title of the pull request.
    pub title: String,
    /// Name of the head branch.
    pub head_ref_name: Option<String>,
    /// SHA of the head commit.
    pub head_sha: Option<String>,
    /// State of the PR (e.g., "open", "closed").
    pub state: Option<String>,
    /// Mergeability status from `GitHub` API.
    pub mergeable: Option<String>,
    /// Body/description of the PR.
    pub body: Option<String>,
    /// ISO 8601 timestamp of last update.
    pub updated_at: Option<String>,
    /// ISO 8601 timestamp when merged, if applicable.
    pub merged_at: Option<String>,
}
```

```rust
pub enum MergeMethod {
    /// Squash all commits into a single commit.
    Squash,
    /// Create a merge commit (traditional merge).
    Merge,
    /// Rebase commits onto the target branch.
    Rebase,
}
```

```rust
pub struct Issue {
    /// Issue number.
    pub number: u64,
    /// Title of the issue.
    pub title: String,
    /// Body/description of the issue.
    pub body: Option<String>,
    /// Labels attached to the issue.
    pub labels: Vec<String>,
    /// State of the issue (e.g., "open", "closed").
    pub state: Option<String>,
    /// ISO 8601 timestamp when the issue was created.
    pub created_at: Option<String>,
}
```

```rust
pub struct ClassifiedPr {
    /// The underlying pull request.
    pub pr: PullRequest,

    /// Aggregate CI status across all checks.
    pub ci_status: CiStatus,

    /// Files changed by this PR (relative paths).
    pub changed_files: Vec<String>,

    /// Prompt number extracted from the PR title or branch name.
    pub prompt_number: Option<u32>,

    /// Whether all changed files fall within the declared blast radius.
    pub blast_radius_ok: bool,

    /// Whether the diff is free of known anti-patterns.
    pub merge_safe: bool,

    /// Whether a `Gate-Passed` trailer was found in any PR commit.
    /// WHY: Local gate enforcement replaces the verify-gate `GitHub` Action.
    /// When CI checks are absent (minutes exhausted), this field allows the
    /// steward to treat the PR as green without depending on `GitHub` CI.
    pub has_gate_trailer: bool,

    /// Suppression findings detected in the PR diff.
    /// WHY: Structural suppression detection distinguishes between `#[allow]`
    /// (discouraged) and `#[expect(..., reason = "...")]` (preferred).
    pub suppression_findings: Vec<SuppressionFinding>,

    /// QA verdict for this PR, if available.
    /// WHY: Tiered merge policy requires QA verdict to distinguish
    /// PASS (auto-merge eligible) from PARTIAL/FAIL (hold/block).
    /// Extracted from the PR body `<!-- qa-verdict: PASS -->` marker or DB.
    pub qa_verdict: Option<QaVerdictStatus>,
}
```

```rust
pub struct SuppressionFinding {
    /// The file where the suppression was found.
    pub file: String,

    /// The line number where the suppression was found (1-indexed).
    pub line: u32,

    /// The kind of suppression detected.
    pub kind: SuppressionKind,

    /// The lint name being suppressed (e.g., `dead_code`, `clippy::unwrap_used`).
    pub lint_name: Option<String>,

    /// The reason provided for the suppression, if any.
    pub reason: Option<String>,
}
```

```rust
pub enum SuppressionKind {
    /// `#[allow(...)]` attribute -- discouraged, use `#[expect]` instead.
    Allow,

    /// `#[expect(...)]` attribute without a reason -- should include justification.
    ExpectNoReason,

    /// `#[expect(..., reason = "...")]` -- preferred form.
    ExpectWithReason,

    /// `#[cfg_attr(..., allow(...))]` -- conditional suppression.
    CfgAttrAllow,

    /// New line added to a lint-ignore file.
    LintIgnoreFile,

    /// `// lint-ignore` inline comment.
    LintIgnoreInline,

    /// `// SAFETY:` or `// INVARIANT:` comment added in a lint-fix PR context.
    /// WHY: LLM workers add these to bypass skip patterns without
    /// the code actually needing a safety/invariant justification.
    StructuredCommentBypass,
}
```

```rust
pub enum QaVerdictStatus {
    /// All acceptance criteria passed.
    Pass,
    /// Some criteria passed, some failed.
    Partial,
    /// At least one criterion actively failed.
    Fail,
    /// Verdict could not be determined (no QA data available).
    Unknown,
}
```

```rust
pub enum MergeTier {
    /// QA PASS + CI green + single-module blast radius -> auto-merge.
    Tier1AutoMerge,
    /// QA PASS + CI green + multi-module blast radius -> merge + notify architect.
    Tier2MergeNotify,
    /// QA PARTIAL or hold flag -> hold for architect review.
    Tier3Hold,
    /// Touches public API surface -> hold for architect review.
    Tier4PublicApi,
    /// QA FAIL or CI failing -> block.
    Tier5Block,
}
```

```rust
pub enum CiStatus {
    /// All checks passed.
    Green,
    /// One or more checks failed.
    Red,
    /// One or more checks are still running.
    Pending,
    /// No checks found on the PR.
    Unknown,
}
```

```rust
pub struct MergeDecision {
    /// PR number this decision applies to.
    pub pr_number: u64,
    /// The action to take (merge, hold, block, etc.).
    pub action: MergeAction,
    /// Human-readable explanation for the decision.
    pub reason: String,
}
```

```rust
pub enum MergeAction {
    /// Safe to merge with the given method.
    Merge(MergeMethod),
    /// Requires LLM review before merging.
    NeedsReview,
    /// Held for architect review (tiered merge policy).
    /// WHY: QA PARTIAL, hold flag, or public API changes require human review.
    HoldForArchitect(String),
    /// CI failed -- queue for automated fixing.
    NeedsFix,
    /// Cannot merge (e.g. merge conflict).
    Blocked(String),
    /// Do not touch (manual PR, external contributor, etc.).
    Skip(String),
}
```

```rust
pub struct MergeOptions {
    /// Report what would happen without making changes.
    pub dry_run: bool,

    /// Require LLM review for all merges (not just flagged ones).
    pub require_review: bool,

    /// Use squash merge (default true).
    pub squash: bool,
}
```

```rust
pub struct MergeResult {
    /// PR number.
    pub pr_number: u64,
    /// The decision that was made.
    pub decision: MergeDecision,
    /// Whether the merge (if attempted) succeeded.
    pub success: bool,
    /// Error message if the merge failed.
    pub error: Option<String>,
}
```

```rust
pub struct StewardResult {
    /// All classified PRs from this pass.
    pub classified: Vec<ClassifiedPr>,

    /// Merge results for PRs that were attempted.
    pub merged: Vec<MergeResult>,

    /// PRs that need CI fixes (red status).
    pub needs_fix: Vec<ClassifiedPr>,

    /// PRs that are blocked with reasons.
    pub blocked: Vec<(u64, String)>,

    /// CI status of the main branch (from pre-flight check).
    /// WHY: Callers need visibility into whether PR fixes were skipped
    /// due to a broken base branch.
    pub main_ci_status: CiStatus,

    /// Whether a mechanical fix was attempted on the main branch.
    pub main_fix_attempted: bool,
}
```

```rust
pub struct CheckRun {
    /// Name of the check (e.g. "build", "test").
    pub name: String,

    /// Status of the check (e.g. "completed", `in_progress`, "queued").
    /// WHY: `GitHub` API uses "status" not "state" for check-runs.
    #[serde(default, alias = "state")]
    pub status: String,

    /// Conclusion of the check (e.g. "success", "failure", "neutral").
    #[serde(default)]
    pub conclusion: Option<String>,
}
```

```rust
pub struct PrFile {
    /// Path of the changed file.
    pub filename: String,
}
```

```rust
pub struct FixResult {
    /// PR number that was fixed.
    pub pr_number: u64,
    /// Individual fixes that were applied.
    pub fixes_applied: Vec<FixApplied>,
    /// Whether CI might still be failing after fixes.
    pub still_failing: bool,
}
```

```rust
pub struct FixApplied {
    /// What kind of fix was applied (format, clippy, etc.).
    pub kind: FixKind,
    /// Files that were changed by this fix.
    pub files_changed: Vec<String>,
    /// Human-readable description of what was done.
    pub details: String,
}
```

```rust
pub enum FixKind {
    /// `cargo fmt --all`
    Format,
    /// `cargo clippy --fix`
    ClippyFix,
    /// Removed unfulfilled `#[expect(...)]` attributes.
    ExpectRemoval,
    /// Trailing whitespace / missing final newline.
    Whitespace,
    /// Injected `Gate-Passed` trailer into HEAD commit.
    GateTrailer,
    /// Regenerated `Cargo.lock` via `cargo generate-lockfile`.
    LockfileRegen,
    /// Resolved training file conflicts via take-theirs.
    TrainingTakeTheirs,
    /// LLM agent applied a semantic fix.
    LlmFix,
}
```

```rust
pub struct CiFailure {
    /// Name of the failing check (e.g. "build", "test").
    pub check_name: String,
    /// Conclusion string (e.g. "failure", `"timed_out"`).
    pub conclusion: String,
    /// Relevant portion of the CI log showing the failure.
    pub log_excerpt: String,
}
```

```rust
pub enum CiFailureKind {
    /// Deterministic fix exists (fmt, clippy --fix, trailer injection).
    Mechanical,
    /// Requires LLM reasoning (type errors, test failures, logic bugs).
    Semantic,
}
```

```rust
pub struct ConflictResult {
    /// PR number.
    pub pr_number: u64,
    /// Whether the conflict was resolved.
    pub resolved: bool,
    /// Strategy used to resolve the conflict.
    pub strategy: ConflictStrategy,
    /// Human-readable details about the resolution.
    pub details: String,
}
```

```rust
pub enum ConflictStrategy {
    /// Resolved via API rebase (fast, free).
    ApiRebase,
    /// Resolved by file-type-specific merge strategies (Rust, TOML, Markdown, JSON).
    FileTypeStrategy,
    /// Resolved by local structured rebase (merge + file-type strategies + push).
    StructuredRebase,
    /// Resolved by an LLM rebase agent.
    LlmRebase,
    /// Skipped because the PR was closed or merged before resolution.
    Skipped,
}
```

## `src/store/fjall_store.rs`

> State persistence layer wrapping a fjall keyspace.
> 
> All dispatch, session, lesson, observation, and CI validation records are
> stored in a dedicated `"energeia"` partition with byte-prefixed keys for
> efficient prefix scans.
```rust
pub struct EnergeiaStore {
    keyspace: Arc<fjall::Keyspace>,
}
```

```rust
impl EnergeiaStore {
    pub fn new (db: &fjall::Database) -> Result<Self>;
    pub fn from_keyspace (keyspace: Arc<fjall::Keyspace>) -> Self;
    pub fn partition_name () -> &'static str;
    pub fn create_session (
        &self,
        dispatch_id: &DispatchId,
        prompt_number: u32,
    ) -> Result<SessionId>;
    pub fn update_session (&self, id: &SessionId, update: SessionUpdate) -> Result<()>;
    pub fn add_lesson (&self, lesson: &NewLesson) -> Result<()>;
    pub fn query_lessons (
        &self,
        source: Option<&str>,
        category: Option<&str>,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LessonRecord>>;
    pub fn add_observation (&self, observation: &NewObservation) -> Result<()>;
    pub fn query_observations (
        &self,
        project: Option<&str>,
        days: Option<u32>,
        limit: usize,
    ) -> Result<Vec<ObservationRecord>>;
    pub fn add_ci_validation (
        &self,
        session_id: &SessionId,
        check_name: &str,
        pr_number: u64,
        status: CiValidationStatus,
        details: Option<String>,
    ) -> Result<()>;
    pub fn add_qa_verdict (
        &self,
        dispatch_id: &DispatchId,
        project: &str,
        verdict: crate::types::QaVerdict,
    ) -> Result<()>;
    pub fn record_training_data (
        &self,
        session: &SessionRecord,
        outcome: &SessionOutcome,
    ) -> Result<Fact>;
    pub fn list_qa_verdicts_for_dispatch (
        &self,
        dispatch_id: &DispatchId,
    ) -> Result<Vec<QaVerdictRecord>>;
    pub fn list_ci_validations_for_session (
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<CiValidationRecord>>;
}
```

## `src/store/records.rs`

```rust
pub struct DispatchRecord {
    /// Unique identifier for this dispatch.
    pub id: DispatchId,
    /// Project slug (owner/repo) this dispatch belongs to.
    pub project: String,
    /// Serialized dispatch specification (JSON).
    pub spec: String,
    /// Current lifecycle status of the dispatch.
    pub status: DispatchStatus,
    /// Timestamp when the dispatch was created.
    pub created_at: jiff::Timestamp,
    /// Timestamp when the dispatch finished, if completed.
    pub finished_at: Option<jiff::Timestamp>,
    /// Total cost in USD across all sessions in this dispatch.
    pub total_cost_usd: f64,
    /// Total number of sessions in this dispatch.
    pub total_sessions: u32,
}
```

```rust
pub enum DispatchStatus {
    /// Dispatch is currently in progress.
    Running,
    /// Dispatch completed successfully.
    Completed,
    /// Dispatch failed or was aborted.
    Failed,
}
```

```rust
pub struct SessionRecord {
    /// Unique identifier for this session.
    pub id: SessionId,
    /// Parent dispatch this session belongs to.
    pub dispatch_id: DispatchId,
    /// Prompt number this session is executing.
    pub prompt_number: u32,
    /// Current execution status of the session.
    pub status: SessionStatus,
    /// Claude Code session identifier, set after agent starts.
    pub session_id: Option<String>,
    /// Cost in USD for this session.
    pub cost_usd: f64,
    /// Number of turns (agent iterations) in this session.
    pub num_turns: u32,
    /// Duration of the session in milliseconds.
    pub duration_ms: u64,
    /// URL of the PR created by this session, if any.
    pub pr_url: Option<String>,
    /// Error message if the session failed.
    pub error: Option<String>,
    /// Timestamp when the session was created.
    pub created_at: jiff::Timestamp,
    /// Timestamp of the last update to this session.
    pub updated_at: jiff::Timestamp,
}
```

```rust
pub struct SessionUpdate {
    /// New status for the session, if changed.
    pub status: Option<SessionStatus>,
    /// Claude Code session identifier, once known.
    pub session_id: Option<String>,
    /// Updated cost in USD.
    pub cost_usd: Option<f64>,
    /// Updated turn count.
    pub num_turns: Option<u32>,
    /// Updated duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// PR URL created by the session.
    pub pr_url: Option<String>,
    /// Error message if the session failed.
    pub error: Option<String>,
}
```

```rust
pub struct LessonRecord {
    /// Source of the lesson (e.g., "steward", "qa").
    pub source: String,
    /// Category for grouping related lessons.
    pub category: String,
    /// The lesson text itself.
    pub lesson: String,
    /// Supporting evidence or context.
    pub evidence: Option<String>,
    /// Project this lesson relates to, if any.
    pub project: Option<String>,
    /// Prompt number this lesson relates to, if any.
    pub prompt_number: Option<u32>,
    /// Timestamp when the lesson was recorded.
    pub created_at: jiff::Timestamp,
}
```

```rust
pub struct NewLesson {
    /// Source of the lesson (e.g., "steward", "qa").
    pub source: String,
    /// Category for grouping related lessons.
    pub category: String,
    /// The lesson text itself.
    pub lesson: String,
    /// Supporting evidence or context.
    pub evidence: Option<String>,
    /// Project this lesson relates to, if any.
    pub project: Option<String>,
    /// Prompt number this lesson relates to, if any.
    pub prompt_number: Option<u32>,
}
```

```rust
pub struct ObservationRecord {
    /// Unique identifier for this observation.
    pub id: String,
    /// Project this observation relates to.
    pub project: String,
    /// Source that captured the observation.
    pub source: String,
    /// Content of the observation.
    pub content: String,
    /// Type of observation (e.g., "bug", "insight").
    pub observation_type: String,
    /// Session ID that produced this observation, if any.
    pub session_id: Option<String>,
    /// Timestamp when the observation was recorded.
    pub created_at: jiff::Timestamp,
}
```

```rust
pub struct NewObservation {
    /// Project this observation relates to.
    pub project: String,
    /// Source that captured the observation.
    pub source: String,
    /// Content of the observation.
    pub content: String,
    /// Type of observation (e.g., "bug", "insight").
    pub observation_type: String,
    /// Session ID that produced this observation, if any.
    pub session_id: Option<String>,
}
```

```rust
pub struct CiValidationRecord {
    /// Session this validation relates to.
    pub session_id: SessionId,
    /// Name of the CI check (e.g., "build", "test").
    pub check_name: String,
    /// PR number that was validated.
    pub pr_number: u64,
    /// Outcome of the validation.
    pub status: CiValidationStatus,
    /// Additional details about the validation result.
    pub details: Option<String>,
    /// Timestamp when the validation was recorded.
    pub validated_at: jiff::Timestamp,
}
```

```rust
pub enum CiValidationStatus {
    /// CI validation passed.
    Pass,
    /// CI validation failed.
    Fail,
}
```

```rust
pub struct QaVerdictRecord {
    /// Parent dispatch this verdict belongs to.
    pub dispatch_id: DispatchId,
    /// Project slug this verdict belongs to.
    pub project: String,
    /// Overall QA verdict.
    pub verdict: QaVerdict,
    /// Timestamp when the verdict was recorded.
    pub recorded_at: jiff::Timestamp,
}
```

```rust
pub struct SessionOutcomeData {
    /// Prompt number this session executed.
    pub prompt_number: u32,
    /// Final status of the session.
    pub status: SessionStatus,
    /// Cost in USD for this session.
    pub cost_usd: f64,
    /// Number of turns (agent iterations) in this session.
    pub num_turns: u32,
    /// Duration of the session in milliseconds.
    pub duration_ms: u64,
    /// URL of the PR created by this session, if any.
    pub pr_url: Option<String>,
    /// Number of QA-driven corrective attempts made for this prompt.
    #[serde(default)]
    pub corrective_attempts: u32,
}
```

## `src/types.rs`

```rust
pub struct DispatchSpec {
    /// Prompt numbers to execute (may be a subset of the full DAG).
    pub prompt_numbers: Vec<u32>,
    /// Project identifier this dispatch belongs to.
    pub project: String,
    /// Optional reference to a prompt DAG for dependency ordering.
    pub dag_ref: Option<String>,
    /// Maximum parallelism (simultaneous sessions). `None` means unlimited.
    pub max_parallel: Option<u32>,
    /// Maximum turns per initial session. `None` delegates to engine defaults.
    pub max_turns: Option<u32>,
}
```

```rust
impl DispatchSpec {
    pub fn new (project: String, prompt_numbers: Vec<u32>) -> Self;
    pub fn with_options (
        project: String,
        prompt_numbers: Vec<u32>,
        dag_ref: Option<String>,
        max_parallel: Option<u32>,
    ) -> Self;
    pub fn with_max_turns (mut self, max_turns: Option<u32>) -> Self;
}
```

```rust
pub struct DispatchResult {
    /// Unique identifier for this dispatch run.
    pub dispatch_id: String,
    /// Per-prompt outcomes in execution order.
    pub outcomes: Vec<SessionOutcome>,
    /// Total cost across all sessions in USD.
    pub total_cost_usd: f64,
    /// Wall-clock duration of the entire dispatch.
    pub duration_ms: u64,
    /// Whether the dispatch was aborted before completing all prompts.
    pub aborted: bool,
    /// Timestamp when the dispatch completed.
    pub completed_at: Timestamp,
}
```

```rust
pub struct SessionOutcome {
    /// The prompt number that was executed.
    pub prompt_number: u32,
    /// Terminal status of the session.
    pub status: SessionStatus,
    /// Agent SDK session identifier, if one was created.
    pub session_id: Option<String>,
    /// Total cost in USD for this session (including resumes).
    pub cost_usd: f64,
    /// Total LLM turns consumed.
    pub num_turns: u32,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Number of times the session was resumed via health checks.
    pub resume_count: u32,
    /// Pull request URL if the session produced one.
    pub pr_url: Option<String>,
    /// Error message if the session failed.
    pub error: Option<String>,
    /// LLM model used for this session (e.g., "claude-3-5-sonnet").
    ///
    /// This is `None` if the model could not be determined from the session.
    pub model: Option<String>,
    /// Blast radius paths from the prompt spec.
    ///
    /// Used for cost attribution to specific modules/features.
    pub blast_radius: Vec<String>,
    /// Number of QA-driven corrective attempts made for this prompt before
    /// this outcome. `0` means this is the original execution.
    #[serde(default)]
    pub corrective_attempts: u32,
    /// Tokens read from the prompt cache on this session.
    #[serde(default)]
    pub cache_hit_tokens: u64,
    /// Tokens written to the prompt cache on this session.
    #[serde(default)]
    pub cache_miss_tokens: u64,
}
```

```rust
pub enum SessionStatus {
    /// Session completed its task successfully.
    Success,
    /// Session failed to complete its task.
    Failed,
    /// Session became stuck (health escalation reached terminal level).
    Stuck,
    /// Session was aborted via cancellation token.
    Aborted,
    /// Session exceeded its budget allocation.
    BudgetExceeded,
    /// Session was skipped (dependency failed or dispatch aborted).
    Skipped,
    /// Infrastructure failure (zero turns, short duration — auth/network issues).
    InfraFailure,
}
```

```rust
pub struct QaResult {
    /// The prompt number that produced the PR.
    pub prompt_number: u32,
    /// Pull request number evaluated.
    pub pr_number: u64,
    /// Overall verdict.
    pub verdict: QaVerdict,
    /// Per-criterion evaluation results.
    pub criteria_results: Vec<CriterionResult>,
    /// Mechanical issues found in the diff.
    pub mechanical_issues: Vec<MechanicalIssue>,
    /// Human-readable reasons for the verdict, derived from failed criteria
    /// and mechanical issues.
    pub reasons: Vec<String>,
    /// Cost in USD for the LLM evaluation.
    pub cost_usd: f64,
    /// Timestamp when the evaluation completed.
    pub evaluated_at: Timestamp,
    /// Whether semantic (LLM-based) evaluation was included in this result.
    ///
    /// When `false`, the verdict reflects mechanical checks only and the
    /// operator should be aware that semantic criteria were not evaluated.
    pub semantic_evaluated: bool,
}
```

```rust
impl QaResult {
    pub fn new (
        prompt_number: u32,
        pr_number: u64,
        verdict: QaVerdict,
        criteria_results: Vec<CriterionResult>,
        mechanical_issues: Vec<MechanicalIssue>,
        reasons: Vec<String>,
        cost_usd: f64,
        evaluated_at: Timestamp,
        semantic_evaluated: bool,
    ) -> Self;
}
```

```rust
pub enum QaVerdict {
    /// All criteria pass, no blocking mechanical issues.
    Pass,
    /// Some criteria fail but the PR is partially acceptable.
    Partial,
    /// Critical criteria fail or blocking mechanical issues found.
    Fail,
}
```

```rust
pub struct CriterionResult {
    /// The acceptance criterion text.
    pub criterion: String,
    /// Whether this criterion was mechanically or semantically evaluated.
    pub classification: CriterionType,
    /// Whether the criterion passed.
    pub passed: bool,
    /// Supporting evidence from the diff or evaluation.
    pub evidence: String,
}
```

```rust
pub enum CriterionType {
    /// Checkable by static analysis (lint, format, blast radius).
    Mechanical,
    /// Requires LLM evaluation of intent and correctness.
    Semantic,
}
```

```rust
pub struct MechanicalIssue {
    /// Category of the issue.
    pub kind: MechanicalIssueKind,
    /// Human-readable description.
    pub message: String,
    /// Optional additional details (file paths, line numbers, etc.).
    pub details: Option<String>,
}
```

```rust
pub enum MechanicalIssueKind {
    /// Changes touch files outside the declared blast radius.
    BlastRadiusViolation,
    /// Known anti-pattern detected in the diff.
    AntiPattern,
    /// Lint check failure.
    LintViolation,
    /// Code formatting violation.
    FormatViolation,
}
```
