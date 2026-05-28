# L3 API Index: dokimion

Crate path: `crates/eval`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/benchmarks/baselines.rs`

```rust
pub struct Baseline {
    /// System name (e.g. "Hindsight", "GPT-4o + memory").
    pub system: &'static str,
    /// Exact-match rate (0.0–1.0) if reported.
    pub exact_match_rate: Option<f64>,
    /// Mean F1 score (0.0–1.0) if reported.
    pub mean_f1: Option<f64>,
    /// Free-form note (e.g. "upper bound: full context at query time").
    pub note: &'static str,
}
```

```rust
pub struct CategoryBaseline {
    /// Category name (e.g. `"single-session-user"`, `"multi_hop"`).
    pub category: &'static str,
    /// Exact-match rate (0.0–1.0) if reported.
    pub exact_match_rate: Option<f64>,
    /// Mean F1 score (0.0–1.0) if reported.
    pub mean_f1: Option<f64>,
}
```

> Baselines for the `LongMemEval` benchmark.
> 
> Paper: *`LongMemEval`: Benchmarking Chat Assistants on Long-Term Interactive
> Memory*, Zhang et al., 2024 (arxiv:2410.10813).
```rust
pub fn longmemeval_baselines () -> Vec<Baseline>
```

> Per-category `LongMemEval` baselines (Hindsight and GPT-4o + memory).
```rust
pub fn longmemeval_category_baselines () -> Vec<(&'static str, Vec<CategoryBaseline>)>
```

> Baselines for the `LoCoMo` benchmark.
> 
> Paper: *Long-Context Conversational Memory (`LoCoMo`)*, Maharana et al.,
> 2024 (arxiv:2402.17753).
```rust
pub fn locomo_baselines () -> Vec<Baseline>
```

> Per-category `LoCoMo` baselines (Hindsight and GPT-4 + memory).
```rust
pub fn locomo_category_baselines () -> Vec<(&'static str, Vec<CategoryBaseline>)>
```

## `src/benchmarks/judge.rs`

```rust
pub struct JudgeScore {
    /// Whether the judge deemed the answer correct.
    pub correct: bool,
    /// Short reasoning provided by the judge.
    pub reasoning: String,
}
```

```rust
pub struct LlmJudgeConfig {
    /// OpenAI-compatible chat completions URL.
    pub endpoint: String,
    /// Model identifier (e.g. "gpt-4o", "claude-3-5-sonnet").
    pub model: String,
    /// Optional API key.
    pub api_key: Option<String>,
    /// Maximum tokens for the judgment response.
    pub max_tokens: u32,
    /// Temperature (default 0.0 for deterministic judging).
    pub temperature: f32,
}
```

> Default LLM-judge model. Overridable via [`LlmJudgeConfig::model`].
```rust
pub const DEFAULT_JUDGE_MODEL: &str = "gpt-4o";
```

```rust
impl LlmJudge {
    pub async fn judge (&self, question: &str, actual: &str, expected: &str) -> Result<JudgeScore>;
}
```

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

```rust
pub fn recall_at_k (retrieved: &[String], relevant: &[String], k: usize) -> f64
```

```rust
pub fn ndcg_at_k (retrieved: &[String], relevant: &[String], k: usize) -> f64
```

## `src/benchmarks/mod.rs`

> Re-export of [`EvalClient`](crate::client::EvalClient) for external use.
> 
> External consumers of the benchmark runner need this to construct a
> runner. The rest of the client API surface is not stable.
```rust
pub type EvalClient = crate::client::EvalClient;
```

```rust
pub struct BenchmarkQuestion {
    // kanon:ignore RUST/primitive-for-domain-id — benchmark question id from external dataset JSON, not a domain newtype
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
pub struct BenchmarkMetadata {
    /// ISO-8601 timestamp when the benchmark run started.
    pub timestamp: String,
    /// Aletheia version string from `/api/health`.
    pub aletheia_version: String,
    // kanon:ignore RUST/primitive-for-domain-id — nous_id deserialized from API response; newtype would require custom Deserialize
    /// Nous agent ID used for the benchmark.
    pub nous_id: String,
    /// Model identifier from the nous agent configuration.
    pub model: String,
    /// Name of the benchmark dataset.
    pub benchmark: String,
    /// Total questions in the dataset.
    pub total_questions: usize,
    /// Number of questions actually evaluated (after `max_questions` limit).
    pub evaluated_questions: usize,
    /// Per-question timeout in seconds.
    pub timeout_secs: u64,
}
```

```rust
pub struct QuestionResult {
    // kanon:ignore RUST/primitive-for-domain-id — benchmark result id mirrors external dataset question id
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
    /// Optional LLM-as-judge score (populated when judge is configured).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub judge_score: Option<judge::JudgeScore>,
    /// Optional retrieval metrics: facts retrieved for the question.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub retrieved_facts: Option<Vec<String>>,
    /// Optional retrieval metric: Recall@k.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub recall_at_k: Option<f64>,
    /// Optional retrieval metric: NDCG@k.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ndcg_at_k: Option<f64>,
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
    /// System and run metadata.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<BenchmarkMetadata>,
    /// Statistical summary with 95% CI for key metrics.
    ///
    /// Populated by calling [`BenchmarkReport::with_statistics`].
    /// Absent in reports produced without statistical analysis.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub statistics: Option<BenchmarkStatistics>,
}
```

```rust
pub struct BenchmarkStatistics {
    /// 95% bootstrap CI lower bound for mean F1 across all questions.
    pub f1_ci_low: f64,
    /// 95% bootstrap CI upper bound for mean F1 across all questions.
    pub f1_ci_high: f64,
    /// 95% bootstrap CI lower bound for exact-match rate.
    pub em_ci_low: f64,
    /// 95% bootstrap CI upper bound for exact-match rate.
    pub em_ci_high: f64,
    /// Number of bootstrap resamples used to compute CI.
    pub n_resamples: usize,
    /// Tool + version string for provenance.
    pub method: String,
}
```

```rust
impl BenchmarkReport {
    pub fn new (benchmark: impl Into<String>, questions: Vec<QuestionResult>) -> Self;
    pub fn with_metadata (
        benchmark: impl Into<String>,
        questions: Vec<QuestionResult>,
        metadata: BenchmarkMetadata,
    ) -> Self;
    pub fn with_statistics (mut self, n_resamples: usize) -> Self;
    pub fn exact_match_rate (&self) -> f64;
    pub fn mean_f1 (&self) -> f64;
    pub fn judge_accuracy (&self) -> Option<f64>;
    pub fn mean_recall_at_k (&self) -> Option<f64>;
    pub fn mean_ndcg_at_k (&self) -> Option<f64>;
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
    // kanon:ignore RUST/primitive-for-domain-id — nous_id deserialized from API response; newtype would require custom Deserialize
    /// Nous ID that will receive the benchmark session.
    pub nous_id: String,
    // kanon:ignore RUST/plain-string-secret — session_key_prefix is a human-readable benchmark key prefix, not a credential
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
    /// Optional LLM-as-judge configuration. When set, each answer is also
    /// evaluated by an external LLM for binary correctness.
    pub judge: Option<judge::LlmJudgeConfig>,
    /// When set, query the knowledge store after ingestion and compute
    /// Recall@k and NDCG@k against the expected answers.
    pub retrieval_k: Option<usize>,
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
    // kanon:ignore RUST/primitive-for-domain-id — API response DTO; newtype would require custom Deserialize
    pub id: String,
    pub model: String,
    pub status: String,
}
```

```rust
pub struct NousStatus {
    // kanon:ignore RUST/primitive-for-domain-id — API response DTO; newtype would require custom Deserialize
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
    // kanon:ignore RUST/primitive-for-domain-id — API response DTO; newtype would require custom Deserialize
    pub id: String,
    // kanon:ignore RUST/primitive-for-domain-id — API response DTO; newtype would require custom Deserialize
    pub nous_id: String,
    // kanon:ignore RUST/plain-string-secret — session_key is an API response DTO field, not a stored credential
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
    // kanon:ignore RUST/primitive-for-domain-id — API response DTO; newtype would require custom Deserialize
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

    /// Statistical computation precondition violated.
    #[snafu(display("stats error: {message}"))]
    Stats {
        /// Human-readable description of the violated precondition.
        message: String,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

> Convenience alias for `Result` with eval's [`Error`] type.
```rust
pub type Result<T> = std::result::Result<T, Error>;
```

## `src/persistence.rs`

```rust
pub struct EvalRecord {
    /// ISO 8601 timestamp of when the evaluation was run.
    pub timestamp: String,
    /// Evaluation category (e.g., "health", "cognitive", "session").
    pub eval_type: String,
    // kanon:ignore RUST/primitive-for-domain-id — scenario_id for JSONL training data output, mirrors external scenario ids
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

> Append evaluation records from a `RunReport` to a JSONL file and write a
> sibling `<path>.meta.json` file with provenance metadata.
> 
> The `.meta.json` file is always overwritten (not appended) because it
> reflects the provenance of the *most recent* batch of records written to
> the JSONL file.
> 
> # Errors
> 
> Returns `Io` if either file cannot be opened or written to.
> Returns `Json` if serialization of records or metadata fails.
```rust
pub fn append_jsonl_stamped (path: &Path, report: &RunReport) -> Result<()>
```

## `src/provider/provider_impl.rs`

> Provider that returns all built-in dokimion scenarios.
> 
> This is the default when no custom provider is specified  -  it wraps
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

```rust
pub fn emit_eval_report (report: &RunReport) -> crate::error::Result<Vec<u8>>
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

> Boxed future returned by scenario `run` methods.
```rust
pub type ScenarioFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
```

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

## `src/stats/bootstrap.rs`

```rust
pub struct BootstrapCi {
    /// Point estimate of the statistic on the original data.
    pub point: f64,
    /// Lower bound of the confidence interval.
    pub ci_low: f64,
    /// Upper bound of the confidence interval.
    pub ci_high: f64,
    /// Confidence level used (e.g. `0.95` for 95%).
    pub confidence: f64,
    /// Number of bootstrap resamples used.
    pub n_resamples: usize,
}
```

```rust
pub fn bootstrap_ci (
    data: &[f64],
    stat_fn: impl Fn(&[f64]) -> f64,
    n: usize,
    seed: u64,
    ci: f64,
) -> Result<BootstrapCi, Error>
```

```rust
pub fn block_bootstrap_ci (
    series: &[f64],
    stat_fn: impl Fn(&[f64]) -> f64,
    block_length: Option<usize>,
    n: usize,
    seed: u64,
    ci: f64,
) -> Result<BootstrapCi, Error>
```

## `src/stats/effect_size.rs`

```rust
pub enum EffectSizeInterpretation {
    /// |d| < 0.2 (Cohen 1988).
    Negligible,
    /// 0.2 ≤ |d| < 0.5.
    Small,
    /// 0.5 ≤ |d| < 0.8.
    Medium,
    /// |d| ≥ 0.8.
    Large,
    /// Fewer than 2 observations in one group.
    InsufficientData,
}
```

```rust
pub struct CohensD {
    /// The Cohen's d value `(mean_a - mean_b) / pooled_sd`.
    pub d: f64,
    /// 95% bootstrap CI lower bound. `None` when CI was not requested.
    pub ci_low: Option<f64>,
    /// 95% bootstrap CI upper bound. `None` when CI was not requested.
    pub ci_high: Option<f64>,
    /// Verbal interpretation of the magnitude.
    pub interpretation: EffectSizeInterpretation,
}
```

```rust
pub fn cohens_d (a: &[f64], b: &[f64], compute_ci: bool, seed: u64) -> CohensD
```

```rust
pub struct EffectWithCi {
    /// The Cohen's d effect size computation.
    pub effect: CohensD,
    /// The mean of group a.
    pub mean_a: f64,
    /// The mean of group b.
    pub mean_b: f64,
    /// Sample sizes.
    pub n_a: usize,
    /// Sample size of group b.
    pub n_b: usize,
    /// Bootstrap CI for the mean of group a.
    pub ci_a: BootstrapCi,
}
```

## `src/stats/fdr.rs`

```rust
pub enum FdrMethod {
    /// Benjamini-Hochberg (1995): controls FDR under independence or PRDS.
    /// Appropriate for independent or positively-correlated tests.
    #[default]
    BenjaminiHochberg,
    /// Benjamini-Yekutieli (2001): controls FDR under arbitrary dependency.
    /// More conservative; use when tests share samples.
    BenjaminiYekutieli,
}
```

```rust
pub fn fdr_correct (p_values: &[f64], method: FdrMethod) -> Result<Vec<f64>, Error>
```

## `src/stats/report.rs`

```rust
pub struct ComparisonReport {
    /// Human-readable label for this comparison.
    pub label: String,
    /// Number of observations in group a (e.g. baseline scores).
    pub n_a: usize,
    /// Number of observations in group b (e.g. candidate scores).
    pub n_b: usize,
    /// Mean of group a.
    pub mean_a: f64,
    /// Mean of group b.
    pub mean_b: f64,
    /// 95% bootstrap CI for the mean of group a.
    pub ci_a: BootstrapCi,
    /// 95% bootstrap CI for the mean of group b.
    pub ci_b: BootstrapCi,
    /// Cohen's d effect size (group a minus group b).
    pub effect: CohensD,
    /// Raw p-value from a sign test on per-question score differences.
    ///
    /// The sign test asks: across all questions, is the fraction where
    /// `score_a > score_b` significantly different from 0.5?  Computed as
    /// the proportion of pairs where group a outperforms group b.
    pub p_raw: f64,
    /// FDR-adjusted p-value. `None` until the caller applies [`fdr_correct`] and
    /// sets this field.
    ///
    /// [`fdr_correct`]: crate::stats::fdr_correct
    pub p_adjusted: Option<f64>,
    /// Whether the result is significant at α=0.05 (using raw p).
    pub significant_raw: Option<bool>,
    /// Whether the result is significant at α=0.05 after FDR correction.
    /// `None` until `p_adjusted` is set.
    pub significant_adjusted: Option<bool>,
}
```

```rust
impl ComparisonReport {
    pub fn set_adjusted_p (&mut self, p_adjusted: f64);
    pub fn is_significant (&self) -> bool;
    pub fn has_practical_effect (&self) -> bool;
}
```

```rust
pub fn comparison_report (
    a: &[f64],
    b: &[f64],
    label: impl Into<String>,
    fdr_adjusted_p: Option<f64>,
) -> Result<ComparisonReport, Error>
```

## `src/tags.rs`

```rust
pub enum TagId {
    /// A scenario category present in the run.
    Category {
        /// The category name (e.g. "health", "session").
        name: String,
    },
    /// An outcome class present among the results.
    Outcome(OutcomeTag),
    /// The run contains at least one scenario requiring authentication.
    RequiresAuth,
    /// The run contains at least one scenario requiring a nous agent.
    RequiresNous,
    /// The run contains at least one scenario with validation criteria.
    HasCriteria,
    /// Wall-clock duration band for the entire run.
    DurationBand(DurationBand),
    /// Number-of-scenarios band for the run.
    SizeBand(SizeBand),
}
```

```rust
pub enum OutcomeTag {
    /// At least one scenario passed.
    Passed,
    /// At least one scenario failed.
    Failed,
    /// At least one scenario was skipped.
    Skipped,
}
```

```rust
pub enum DurationBand {
    /// Under 1 second.
    Low,
    /// 1 second to 1 minute.
    Medium,
    /// Over 1 minute.
    High,
}
```

```rust
pub enum SizeBand {
    /// No scenarios in the run.
    Empty,
    /// A single scenario.
    Single,
    /// 2–5 scenarios.
    Small,
    /// 6–20 scenarios.
    Medium,
    /// More than 20 scenarios.
    Large,
}
```

```rust
pub fn tag_eval_result (report: &RunReport) -> Vec<TagId>
```
