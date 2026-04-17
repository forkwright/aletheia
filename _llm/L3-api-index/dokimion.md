# L3 API Index: dokimion

Crate path: `crates/eval`

Public API signatures extracted from source. Doc comments shown above each signature.
For implementation context, read the source directly (`L4`).

## `src/benchmarks/locomo.rs`

```rust
pub struct LocomoDataset {
    conversations: Vec<LocomoConversation>,
}
```

```rust
impl LocomoDataset {
    pub async fn from_path (path: impl AsRef<Path> + Send) -> io::Result<Self>;
    pub fn from_bytes (bytes: &[u8]) -> io::Result<Self>;
    pub fn question_count (&self) -> usize;
}
```

## `src/benchmarks/longmemeval.rs`

```rust
pub struct LongMemEvalDataset {
    items: Vec<LongMemEvalItem>,
}
```

```rust
impl LongMemEvalDataset {
    pub async fn from_path (path: impl AsRef<Path> + Send) -> io::Result<Self>;
    pub fn from_bytes (bytes: &[u8]) -> io::Result<Self>;
}
```

## `src/benchmarks/metrics.rs`

```rust
pub struct BenchmarkScore {
    /// Exact match: normalized strings are equal.
    pub exact_match: bool,
    /// Token-level F1 score in [0.0, 1.0].
    pub f1: f64,
    /// Whether any expected answer is a substring of the actual answer.
    pub contains: bool,
}
```

```rust
impl BenchmarkScore {
    pub fn zero () -> Self;
}
```

```rust
pub fn score_answer (actual: &str, expected: &[String]) -> BenchmarkScore
```

## `src/benchmarks/mod.rs`

```rust
pub struct BenchmarkQuestion {
    /// Unique identifier for this question within the benchmark.
    pub id: String,
    /// The conversations (sessions) to ingest before asking this question.
    ///
    /// Each session is a list of turns; each turn is (role, content).
    pub sessions: Vec<Vec<(String, String)>>,
    /// The question text to ask after ingestion.
    pub question: String,
    /// The ground-truth answer(s). Multiple acceptable answers may be listed.
    pub expected_answers: Vec<String>,
    /// Category label for per-ability scoring (e.g. "temporal", "multi-session").
    pub category: String,
}
```

> A memory benchmark dataset: a collection of questions.
```rust
pub trait MemoryBenchmark {
    fn name (&self) -> &'static str;
    fn questions (&self) -> Box<dyn Iterator<Item = BenchmarkQuestion> + '_>;
    fn len (&self) -> usize;
    fn is_empty (&self) -> bool; // default impl
}
```

```rust
pub struct QuestionResult {
    /// Question id.
    pub id: String,
    /// Category.
    pub category: String,
    /// The answer produced by aletheia.
    pub actual_answer: String,
    /// The expected answers (ground truth, may have multiple valid forms).
    pub expected_answers: Vec<String>,
    /// Best score across all expected answers.
    pub score: BenchmarkScore,
}
```

```rust
pub struct BenchmarkReport {
    /// Benchmark name.
    pub benchmark: String,
    /// Total questions scored.
    pub total: usize,
    /// Per-question results.
    pub questions: Vec<QuestionResult>,
}
```

```rust
impl BenchmarkReport {
    pub fn new (benchmark: impl Into<String>, questions: Vec<QuestionResult>) -> Self;
    pub fn exact_match_rate (&self) -> f64;
    pub fn mean_f1 (&self) -> f64;
    pub fn per_category (&self) -> Vec<(String, f64, f64)>;
}
```

```rust
pub fn download_instructions () -> &'static str
```

> Load a `LongMemEval` dataset from a JSON file on disk.
> 
> # Errors
> 
> Returns an error if the file cannot be read or the JSON is not in the
> expected `LongMemEval` format.
```rust
pub async fn load_longmemeval (
    path: impl AsRef<Path> + Send,
) -> std::io::Result<longmemeval::LongMemEvalDataset>
```

> Load a `LoCoMo` dataset from a JSON file on disk.
> 
> # Errors
> 
> Returns an error if the file cannot be read or the JSON is not in the
> expected `LoCoMo` format.
```rust
pub async fn load_locomo (path: impl AsRef<Path> + Send) -> std::io::Result<locomo::LocomoDataset>
```

## `src/benchmarks/runner.rs`

```rust
pub struct BenchmarkRunnerConfig {
    /// Nous ID that will receive the benchmark session.
    pub nous_id: String,
    /// Prefix for `session_key` so multiple runs don't collide.
    pub session_key_prefix: String,
    /// Per-question timeout. If the assistant hasn't emitted `message_complete`
    /// by this deadline, the question is scored as "no answer" (zero).
    pub question_timeout: Duration,
    /// Maximum questions to evaluate. `None` means all questions.
    /// Useful for smoke tests (`max_questions = Some(5)`).
    pub max_questions: Option<usize>,
    /// When true, close sessions after each question to reset memory state.
    /// When false, all questions share one session (simulates continuous memory).
    pub close_between_questions: bool,
}
```

> Runs a memory benchmark against a live aletheia instance.
```rust
pub struct BenchmarkRunner {
    client: EvalClient,
    config: BenchmarkRunnerConfig,
}
```

```rust
impl BenchmarkRunner {
    pub fn new (client: EvalClient, config: BenchmarkRunnerConfig) -> Self;
    pub async fn run (&self, benchmark: &dyn MemoryBenchmark) -> Result<BenchmarkReport>;
}
```

## `src/client.rs`

> HTTP client for a running Aletheia instance.
```rust
pub struct EvalClient {
    http: reqwest::Client,
    base_url: String,
    token: Option<SecretString>,
}
```

```rust
impl EvalClient {
    pub fn new (base_url: impl Into<String>, token: Option<String>) -> Self;
    pub async fn health (&self) -> Result<HealthResponse>;
    pub async fn list_nous (&self) -> Result<Vec<NousSummary>>;
    pub async fn get_nous (&self, id: &str) -> Result<NousStatus>;
    pub async fn create_session (
        &self,
        nous_id: &str,
        session_key: &str,
    ) -> Result<SessionResponse>;
    pub async fn get_session (&self, id: &str) -> Result<SessionResponse>;
    pub async fn close_session (&self, id: &str) -> Result<()>;
    pub async fn send_message (
        &self,
        session_id: &str,
        content: &str,
    ) -> Result<Vec<ParsedSseEvent>>;
    pub async fn get_history (&self, session_id: &str) -> Result<HistoryResponse>;
    pub async fn search_knowledge (
        &self,
        query: &str,
        nous_id: &str,
        limit: u32,
    ) -> Result<KnowledgeSearchResponse>;
    pub async fn raw_get (&self, path: &str) -> Result<reqwest::Response>;
    pub async fn raw_post (
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response>;
    pub async fn raw_get_with_token (&self, path: &str, token: &str) -> Result<reqwest::Response>;
}
```

```rust
pub enum InstanceStatus {
    /// Instance is fully operational.
    Healthy,
    /// Instance is running but one or more checks are failing.
    Degraded,
    /// Catch-all for future or unexpected status strings.
    #[serde(untagged)]
    Unknown(String),
}
```

```rust
pub enum SessionStatus {
    /// Session is open and accepting messages.
    Active,
    /// Session has been closed and is read-only.
    Archived,
    /// Catch-all for future or unexpected status strings.
    #[serde(untagged)]
    Unknown(String),
}
```

```rust
pub enum MessageRole {
    /// Message sent by the user.
    User,
    /// Message generated by the assistant.
    Assistant,
    /// Tool result message.
    Tool,
    /// Catch-all for future or unexpected role strings.
    #[serde(untagged)]
    Unknown(String),
}
```

```rust
pub struct HealthResponse {
    pub status: InstanceStatus,
    pub version: String,
    pub uptime_seconds: u64,
    pub checks: Vec<HealthCheck>,
}
```

```rust
pub struct HealthCheck {
    pub name: String,
    pub status: String,
    pub message: Option<String>,
}
```

```rust
pub struct NousListResponse {
    pub nous: Vec<NousSummary>,
}
```

```rust
pub struct NousSummary {
    pub id: String,
    pub model: String,
    pub status: String,
}
```

```rust
pub struct NousStatus {
    pub id: String,
    pub model: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    #[serde(default)]
    pub thinking_enabled: bool,
    #[serde(default)]
    pub thinking_budget: u32,
    #[serde(default)]
    pub max_tool_iterations: u32,
    #[serde(default)]
    pub status: String,
}
```

```rust
pub struct SessionResponse {
    pub id: String,
    pub nous_id: String,
    pub session_key: String,
    pub status: SessionStatus,
    pub model: Option<String>,
    pub message_count: i64,
    pub token_count_estimate: i64,
    pub created_at: String,
    pub updated_at: String,
}
```

```rust
pub struct HistoryResponse {
    pub messages: Vec<HistoryMessage>,
}
```

```rust
pub struct HistoryMessage {
    pub id: i64,
    pub seq: i64,
    pub role: MessageRole,
    pub content: String,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub created_at: String,
}
```

```rust
pub struct KnowledgeSearchResponse {
    /// Matching facts ordered by relevance.
    #[serde(default)]
    pub facts: Vec<KnowledgeFact>,
}
```

```rust
pub struct KnowledgeFact {
    /// Unique fact identifier.
    pub id: String,
    /// Fact content text.
    #[serde(default)]
    pub content: String,
    /// Confidence score (0.0 to 1.0).
    #[serde(default)]
    pub confidence: f64,
}
```

## `src/error.rs`

```rust
pub enum Error {
    /// HTTP request failed.
    #[snafu(display("HTTP request failed: {source}"))]
    Http {
        /// Underlying reqwest error.
        source: reqwest::Error,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Unexpected HTTP status from the server.
    #[snafu(display("unexpected status {status} from {endpoint}: {body}"))]
    UnexpectedStatus {
        /// The endpoint URL that returned the unexpected status.
        endpoint: String,
        /// HTTP status code that was returned.
        status: u16,
        /// Response body (for diagnostic context).
        body: String,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// SSE stream parse error.
    #[snafu(display("SSE parse error: {message}"))]
    SseParse {
        /// Parser failure detail.
        message: String,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Scenario assertion failed.
    #[snafu(display("assertion failed: {message}"))]
    Assertion {
        /// Assertion failure detail.
        message: String,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JSON serialization or deserialization failed.
    #[snafu(display("JSON error: {source}"))]
    Json {
        /// Underlying `serde_json` error.
        source: serde_json::Error,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Scenario exceeded the configured timeout.
    #[snafu(display("timeout after {elapsed_ms}ms"))]
    Timeout {
        /// Elapsed time in milliseconds when the timeout fired.
        elapsed_ms: u64,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// No agents are registered on the target instance.
    #[snafu(display("no agents available: agent list is empty"))]
    NoAgentsAvailable {
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// File I/O error during result persistence.
    #[snafu(display("I/O error: {source}"))]
    Io {
        /// Underlying `std::io` error.
        source: std::io::Error,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Benchmark question failed to produce a scorable answer.
    #[snafu(display("benchmark error: {message}"))]
    Benchmark {
        /// Human-readable failure detail.
        message: String,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

## `src/persistence.rs`

```rust
pub struct EvalRecord {
    /// ISO 8601 timestamp of when the evaluation was run.
    pub timestamp: String,
    /// Evaluation category (e.g., "health", "cognitive", "session").
    pub eval_type: String,
    /// Scenario identifier.
    pub scenario_id: String,
    /// Whether the scenario passed.
    pub passed: bool,
    /// Duration in milliseconds (0 for skipped scenarios).
    pub duration_ms: u64,
    /// Outcome kind: "passed", "failed", or "skipped".
    pub outcome: String,
    /// Error message or skip reason, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
```

```rust
pub fn records_from_report (report: &RunReport) -> Vec<EvalRecord>
```

> Append evaluation records to a JSONL file, creating it if necessary.
> 
> # Errors
> 
> Returns `Io` if the file cannot be opened or written to.
> Returns `Json` if a record cannot be serialized.
```rust
pub fn append_jsonl (path: &Path, records: &[EvalRecord]) -> Result<()>
```

## `src/provider/provider_impl.rs`

> Provider that returns all built-in dokimion scenarios.
> 
> This is the default when no custom provider is specified — it wraps
> [`scenarios::all_scenarios()`](crate::scenarios::all_scenarios).
```rust
pub struct BuiltinProvider;
```

> Combines multiple providers into a single scenario set.
> 
> Scenarios are collected in provider order. Deduplication is the caller's
> responsibility (scenario IDs are not enforced unique across providers).
```rust
pub struct CompositeProvider {
    providers: Vec<Box<dyn EvalProvider>>,
    name: String,
}
```

```rust
impl CompositeProvider {
    pub fn new (providers: Vec<Box<dyn EvalProvider>>) -> Self;
}
```

## `src/provider.rs`

> Source of evaluation scenarios.
> 
> Implementations decide which scenarios to include. The runner calls
> [`provide`] once at the start of a run and executes the returned set.
```rust
pub trait EvalProvider : Send + Sync {
    fn provide (&self) -> Vec<Box<dyn Scenario>>;
    fn name (&self) -> &str;
}
```

## `src/report.rs`

```rust
pub fn print_report (report: &RunReport, base_url: &str)
```

```rust
pub fn print_report_json (report: &RunReport)
```

## `src/runner.rs`

> Configuration for a scenario run.
```rust
pub struct RunConfig {
    /// Base URL of the target instance.
    pub base_url: String,
    /// Bearer token for authenticated endpoints.
    pub token: Option<SecretString>,
    /// Substring filter on scenario IDs.
    pub filter: Option<String>,
    /// Exact-match filter on scenario category. When set, only scenarios with
    /// `meta().category == this` are run. Useful for tests that want to run
    /// "session" CRUD scenarios without also pulling in `canary-session`
    /// scenarios that share a substring with the id-based filter.
    pub category_filter: Option<String>,
    /// Stop on first failure.
    pub fail_fast: bool,
    /// Per-scenario timeout in seconds.
    pub timeout_secs: u64,
    /// Emit JSON instead of formatted output.
    pub json_output: bool,
}
```

> Aggregated results from a full eval run.
```rust
pub struct RunReport {
    /// Number of scenarios that passed.
    pub passed: usize,
    /// Number of scenarios that failed.
    pub failed: usize,
    /// Number of scenarios that were skipped.
    pub skipped: usize,
    /// Total wall-clock duration of the run.
    pub total_duration: Duration,
    /// Per-scenario results in run order.
    pub results: Vec<ScenarioResult>,
}
```

> Runs behavioral scenarios against a live Aletheia instance.
```rust
pub struct ScenarioRunner {
    config: RunConfig,
    client: EvalClient,
    provider: Box<dyn EvalProvider>,
}
```

```rust
impl ScenarioRunner {
    pub fn new (config: RunConfig) -> Self;
    pub fn with_provider (config: RunConfig, provider: Box<dyn EvalProvider>) -> Self;
    pub async fn run (&self) -> RunReport;
}
```

## `src/scenario.rs`

```rust
pub struct ScenarioMeta {
    /// Unique identifier (e.g., "health-returns-ok").
    pub id: &'static str,
    /// Human-readable description.
    pub description: &'static str,
    /// Category for grouping in output (e.g., "health", "session").
    pub category: &'static str,
    /// Whether this scenario requires an auth token.
    pub requires_auth: bool,
    /// Whether this scenario requires at least one configured nous.
    pub requires_nous: bool,
    /// Optional substring that the response text must contain.
    pub expected_contains: Option<&'static str>,
    /// Optional regex pattern that the response text must match.
    pub expected_pattern: Option<&'static str>,
}
```

```rust
pub enum ScenarioOutcome {
    /// Scenario completed within timeout without errors.
    Passed {
        /// Wall-clock execution time.
        duration: Duration,
    },
    /// Scenario returned an error or assertion failed.
    Failed {
        /// Wall-clock execution time.
        duration: Duration,
        /// The error that caused failure.
        error: Error,
    },
    /// Scenario was not run (e.g. missing auth token or nous).
    Skipped {
        /// Human-readable reason for skipping.
        reason: String,
    },
}
```

```rust
impl ScenarioOutcome {
    pub fn is_passed (&self) -> bool;
    pub fn is_failed (&self) -> bool;
}
```

```rust
pub struct ScenarioResult {
    /// Metadata describing the scenario.
    pub meta: ScenarioMeta,
    /// Outcome of the run.
    pub outcome: ScenarioOutcome,
}
```

> A behavioral evaluation scenario run against a live instance.
```rust
pub trait Scenario : Send + Sync {
    fn meta (&self) -> ScenarioMeta;
    fn run <'a> (&'a self, client: &'a EvalClient) -> ScenarioFuture<'a>;
}
```

## `src/scenarios/canary.rs`

> Provider that returns all canary scenarios for regression testing.
```rust
pub struct CanaryProvider;
```

```rust
pub fn canary_scenarios () -> Vec<Box<dyn Scenario>>
```

## `src/scenarios/mod.rs`

```rust
pub fn all_scenarios () -> Vec<Box<dyn Scenario>>
```

## `src/sse.rs`

```rust
pub struct ParsedSseEvent {
    /// SSE event type tag (e.g. `"text_delta"`, `"message_complete"`).
    pub event_type: String,
    /// Parsed JSON payload of the event.
    pub data: serde_json::Value,
}
```

```rust
pub async fn parse_sse_stream (response: reqwest::Response) -> Result<Vec<ParsedSseEvent>>
```

```rust
pub struct UsageData {
    /// Number of input tokens consumed by the request.
    pub input_tokens: u64,
    /// Number of output tokens generated by the response.
    pub output_tokens: u64,
}
```

## `src/triggers.rs`

```rust
pub enum TriggerSchedule {
    /// Run on every deployment.
    OnDeploy,
    /// Run daily.
    Daily,
    /// Run weekly.
    Weekly,
    /// Custom cron expression.
    Cron(String),
}
```

```rust
pub struct EvalTrigger {
    /// Scenario ID pattern to match (substring filter).
    pub scenario_pattern: String,
    /// When to run this evaluation.
    pub schedule: TriggerSchedule,
    /// Whether this trigger is active.
    pub enabled: bool,
}
```

```rust
pub struct TriggerConfig {
    /// List of evaluation triggers.
    pub triggers: Vec<EvalTrigger>,
}
```

```rust
impl TriggerConfig {
    pub fn default_config () -> Self;
}
```
