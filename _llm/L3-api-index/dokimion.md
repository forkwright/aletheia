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
    /// Judge execution status.
    #[serde(default)]
    pub status: JudgeStatus,
    /// Error detail when the judge did not produce a parsed score.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error_message: Option<String>,
    /// Provider and parsing provenance for the judge call.
    pub provenance: JudgeProvenance,
}
```

```rust
pub enum JudgeStatus {
    /// The judge returned a parsed judgment.
    #[default]
    Scored,
    /// The judge attempt failed or returned unparseable/refusal data.
    Error,
}
```

```rust
impl JudgeStatus {
    pub fn is_scored (self) -> bool;
}
```

```rust
pub enum JudgeParseStatus {
    /// Provider response parsed into a judge verdict.
    Parsed,
    /// The provider returned a non-success HTTP status.
    HttpError,
    /// The HTTP request or body read failed before parsing.
    TransportError,
    /// The request exceeded the configured judge timeout.
    Timeout,
    /// The provider response JSON lacked message content.
    MissingContent,
    /// The provider or judge JSON was malformed.
    MalformedJson,
    /// The provider returned a refusal instead of judgment content.
    Refusal,
}
```

```rust
pub struct JudgeUsage {
    /// Prompt/input tokens.
    pub prompt_tokens: u64,
    /// Completion/output tokens.
    pub completion_tokens: u64,
    /// Total tokens.
    pub total_tokens: u64,
}
```

```rust
pub struct JudgeProvenance {
    /// OpenAI-compatible endpoint.
    pub endpoint: String,
    /// Judge model.
    pub model: String,
    /// SHA-256 hash of the full user prompt sent to the judge.
    pub prompt_sha256: String,
    /// SHA-256 hash of the raw provider response body.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub raw_response_sha256: Option<String>,
    /// Body reference for external storage; currently the response hash URN.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub raw_response_body_ref: Option<String>,
    /// Provider request ID from headers or response JSON.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub request_id: Option<String>,
    /// Provider token usage, when reported.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub usage: Option<JudgeUsage>,
    /// HTTP status returned by the provider.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider_status: Option<u16>,
    /// Parse/provider outcome.
    pub parse_status: JudgeParseStatus,
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
    /// Explicit HTTP timeout for one judge request.
    pub timeout: Duration,
}
```

> Default LLM-judge model. Overridable via [`LlmJudgeConfig::model`].
```rust
pub const DEFAULT_JUDGE_MODEL: &str = "gpt-4o";
```

```rust
impl LlmJudge {
    pub async fn judge (&self, question: &str, actual: &str, expected: &[String]) -> JudgeScore;
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
    pub async fn from_path_with_options (
        path: impl AsRef<Path> + Send,
        mut options: BenchmarkValidationOptions,
    ) -> io::Result<(Self, BenchmarkValidationReport)>;
    pub fn from_bytes (bytes: &[u8]) -> io::Result<Self>;
    pub fn from_bytes_with_options (
        bytes: &[u8],
        options: &BenchmarkValidationOptions,
    ) -> io::Result<(Self, BenchmarkValidationReport)>;
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
    pub async fn from_path_with_options (
        path: impl AsRef<Path> + Send,
        mut options: BenchmarkValidationOptions,
    ) -> io::Result<(Self, BenchmarkValidationReport)>;
    pub fn from_bytes (bytes: &[u8]) -> io::Result<Self>;
    pub fn from_bytes_with_options (
        bytes: &[u8],
        options: &BenchmarkValidationOptions,
    ) -> io::Result<(Self, BenchmarkValidationReport)>;
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
pub fn normalized_content_ref (content: &str) -> String
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
    /// Expected evidence or fact references supplied by the source dataset.
    pub expected_evidence_refs: Vec<String>,
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
    /// SHA-256 hash of the dataset file, when the runner can read it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub dataset_hash: Option<String>,
    /// Git SHA of the build or invocation, when known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub git_sha: Option<String>,
    /// Whether malformed or incomplete dataset records were allowed.
    #[serde(default)]
    pub dataset_best_effort: bool,
    /// Dataset validation diagnostics captured before execution.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub dataset_validation: Option<BenchmarkValidationReport>,
}
```

```rust
pub enum QuestionStatus {
    /// The question produced an answer and was included in score denominators.
    #[default]
    Scored,
    /// The benchmark pipeline failed before a scorable answer was available.
    Error,
    /// The benchmark question exceeded its configured timeout.
    Timeout,
    /// The model returned an empty answer.
    NoAnswer,
}
```

```rust
impl QuestionStatus {
    pub fn is_scored (self) -> bool;
}
```

```rust
pub struct QuestionResult {
    // kanon:ignore RUST/primitive-for-domain-id — benchmark result id mirrors external dataset question id
    /// Question id.
    pub id: String,
    /// Category.
    pub category: String,
    /// Execution status for this question.
    #[serde(default)]
    pub status: QuestionStatus,
    /// Error or timeout detail when the question was not scorable.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error_message: Option<String>,
    /// The answer produced by aletheia.
    pub actual_answer: String,
    /// The expected answers (ground truth, may have multiple valid forms).
    pub expected_answers: Vec<String>,
    /// Expected evidence/fact references from the dataset, when available.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub expected_evidence_refs: Vec<String>,
    /// Best score across all expected answers.
    pub score: BenchmarkScore,
    /// Optional LLM-as-judge score (populated when judge is configured).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub judge_score: Option<judge::JudgeScore>,
    /// Optional retrieval metrics: facts retrieved for the question.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub retrieved_facts: Option<Vec<RetrievedFact>>,
    /// Retrieval scoring basis and relevant refs used for metrics.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub retrieval_scoring: Option<RetrievalScoring>,
    /// Optional retrieval metric: Recall@k.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub recall_at_k: Option<f64>,
    /// Optional retrieval metric: NDCG@k.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ndcg_at_k: Option<f64>,
}
```

```rust
pub struct RetrievedFact {
    /// Fact ID returned by the knowledge API, when present.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<String>,
    /// Stable display reference for this retrieved fact.
    pub reference: String,
    /// Knowledge API relevance score.
    pub score: f64,
    /// Stored fact confidence.
    pub confidence: f64,
    /// SHA-256 hash of the fact content.
    pub content_sha256: String,
}
```

```rust
pub enum RetrievalScoringMode {
    /// Dataset evidence/fact refs were used as the relevance set.
    EvidenceId,
    /// Dataset lacks evidence refs; normalized content hashes were used.
    NormalizedContent,
}
```

```rust
pub struct RetrievalScoring {
    /// Relevance basis used for Recall@k and NDCG@k.
    pub mode: RetrievalScoringMode,
    /// Whether the normalized-content fallback was used.
    pub fallback_used: bool,
    /// Relevant refs compared against retrieved facts.
    pub relevant_refs: Vec<String>,
}
```

```rust
pub struct BenchmarkReport {
    /// Benchmark name.
    pub benchmark: String,
    /// Total questions attempted.
    pub total: usize,
    /// Questions included in score denominators.
    pub scored: usize,
    /// Questions that failed before producing a scorable answer.
    pub errors: usize,
    /// Questions that exceeded the per-question timeout.
    pub timeouts: usize,
    /// Questions that returned an empty answer.
    pub no_answers: usize,
    /// LLM judge denominator summary, when judge scoring was attempted.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub judge_summary: Option<JudgeSummary>,
    /// Per-question results.
    pub questions: Vec<QuestionResult>,
    /// Shared provenance envelope for this benchmark run.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provenance: Option<EvalProvenance>,
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
pub struct JudgeSummary {
    /// Questions for which judge scoring was attempted.
    pub attempted: usize,
    /// Judge attempts that returned a parsed judgment.
    pub scored: usize,
    /// Judge attempts that failed, timed out, refused, or returned malformed data.
    pub errors: usize,
    /// Parsed judge judgments marked correct.
    pub correct: usize,
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
    pub fn with_provenance (mut self, provenance: EvalProvenance) -> Self;
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

> Load and validate a `LongMemEval` dataset from a JSON file on disk.
> 
> # Errors
> 
> Returns an error if the file cannot be read, parsed, or validated.
```rust
pub async fn load_longmemeval_with_options (
    path: impl AsRef<Path> + Send,
    options: BenchmarkValidationOptions,
) -> std::io::Result<(longmemeval::LongMemEvalDataset, BenchmarkValidationReport)>
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

> Load and validate a `LoCoMo` dataset from a JSON file on disk.
> 
> # Errors
> 
> Returns an error if the file cannot be read, parsed, or validated.
```rust
pub async fn load_locomo_with_options (
    path: impl AsRef<Path> + Send,
    options: BenchmarkValidationOptions,
) -> std::io::Result<(locomo::LocomoDataset, BenchmarkValidationReport)>
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
    /// Shared provenance envelope for the benchmark run.
    pub provenance: EvalProvenance,
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

## `src/benchmarks/validation.rs`

```rust
pub struct BenchmarkValidationOptions {
    /// Dataset path for diagnostics.
    pub dataset_path: Option<String>,
    /// Downgrade incomplete records and unknown categories to warnings.
    pub allow_best_effort: bool,
    /// Require question-level retrieval evidence references.
    pub require_retrieval_evidence: bool,
}
```

```rust
impl BenchmarkValidationOptions {
    pub fn strict () -> Self;
    pub fn strict_for_path (path: impl Into<String>) -> Self;
}
```

```rust
pub struct BenchmarkValidationReport {
    /// Dataset name.
    pub dataset: String,
    /// Dataset path used for diagnostics.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub dataset_path: Option<String>,
    /// Whether best-effort validation was requested.
    pub best_effort: bool,
    /// Whether retrieval evidence refs were required.
    pub require_retrieval_evidence: bool,
    /// Fatal validation errors.
    #[serde(default)]
    pub errors: Vec<BenchmarkValidationIssue>,
    /// Best-effort validation warnings.
    #[serde(default)]
    pub warnings: Vec<BenchmarkValidationIssue>,
}
```

```rust
impl BenchmarkValidationReport {
    pub fn new (dataset: impl Into<String>, options: &BenchmarkValidationOptions) -> Self;
    pub fn error (
        &mut self,
        record_id: Option<String>,
        question_id: Option<String>,
        field: impl Into<String>,
        message: impl Into<String>,
    );
    pub fn issue (
        &mut self,
        options: &BenchmarkValidationOptions,
        record_id: Option<String>,
        question_id: Option<String>,
        field: impl Into<String>,
        message: impl Into<String>,
    );
    pub fn into_result (self) -> io::Result<Self>;
    pub fn error_summary (&self) -> String;
}
```

```rust
pub struct BenchmarkValidationIssue {
    /// Dataset path that contained the invalid record.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub dataset_path: Option<String>,
    /// Dataset record/conversation identifier when known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub record_id: Option<String>,
    /// Question identifier when known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub question_id: Option<String>,
    /// Invalid field name.
    pub field: String,
    /// Human-readable diagnostic.
    pub message: String,
}
```

```rust
pub fn clean_refs (refs: &[String]) -> Vec<String>
```

> Deserialize evidence refs from absent/null, a string, or a string array.
> 
> # Errors
> 
> Returns a serde error when the field is neither a string nor a string array.
```rust
pub fn deserialize_string_list <'de, D> (deserializer: D) -> Result<Vec<String>, D::Error> where
    D: serde::Deserializer<'de>,
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
    #[serde(default, alias = "results")]
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
    /// Search relevance score.
    #[serde(default)]
    pub score: f64,
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
    /// Stable identifier for the eval run.
    pub eval_run_id: String,
    /// Provenance envelope for the run.
    pub provenance: EvalProvenance,
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
    /// Structured sub-results for multi-probe scenarios.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub sub_results: Vec<crate::scenario::ScenarioSubResult>,
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

## `src/provenance.rs`

```rust
pub struct EvalProvenance {
    /// Stable identifier for this eval run.
    // kanon:ignore RUST/primitive-for-domain-id — eval_run_id is an opaque external run handle, not an internal domain newtype
    pub eval_run_id: String,
    /// Schema version of this provenance envelope.
    pub schema_version: u32,
    /// Version of the `dokimion` crate that produced the run.
    pub dokimion_version: String,
    /// Git commit SHA of the running binary, when available.
    pub git_sha: Option<String>,
    /// ISO-8601 timestamp when the run started.
    pub started_at: String,
    /// ISO-8601 timestamp when the run finished, if known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub finished_at: Option<String>,
    /// CLI arguments with secret-bearing values redacted.
    pub redacted_args: Vec<String>,
    /// SHA-256 hash of the resolved run configuration.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub config_hash: Option<String>,
    /// Base URL of the target instance.
    pub target_base_url: String,
    /// Target identity (e.g. version from `/api/health`), when available.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub target_identity: Option<String>,
    /// Opaque model audit reference.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model_ref: Option<String>,
    /// Opaque provider audit reference.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider_ref: Option<String>,
    /// Opaque prompt audit reference.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub prompt_ref: Option<String>,
    /// Opaque tool-surface audit reference.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_ref: Option<String>,
    /// Opaque memory-system audit reference.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub memory_ref: Option<String>,
    /// Hash of the scenario suite or benchmark dataset that was executed.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub scenario_suite_hash: Option<String>,
}
```

```rust
impl EvalProvenance {
    pub fn new (eval_run_id: impl Into<String>, target_base_url: impl Into<String>) -> Self;
    pub fn finished (mut self) -> Self;
    pub fn with_git_sha (mut self, git_sha: impl Into<String>) -> Self;
    pub fn with_redacted_args (mut self, args: &[String]) -> Self;
    pub fn with_config_hash (mut self, hash: impl Into<String>) -> Self;
    pub fn with_target_identity (mut self, identity: impl Into<String>) -> Self;
    pub fn with_audit_refs (
        mut self,
        model: Option<String>,
        provider: Option<String>,
        prompt: Option<String>,
        tool: Option<String>,
        memory: Option<String>,
    ) -> Self;
    pub fn with_scenario_suite_hash (mut self, hash: impl Into<String>) -> Self;
}
```

```rust
pub fn generate_eval_run_id () -> String
```

```rust
pub fn sha256_hex (bytes: &[u8]) -> String
```

```rust
pub fn sha256_hex_str (s: &str) -> String
```

```rust
pub fn redact_args (args: &[String]) -> Vec<String>
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
    /// Durable provenance envelope for this run.
    pub provenance: EvalProvenance,
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
    /// Durable provenance envelope for this run.
    pub provenance: EvalProvenance,
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
pub type ScenarioFuture<'a> = Pin<Box<dyn Future<Output = ScenarioRunOutcome> + Send + 'a>>;
```

```rust
pub enum ScenarioClassification {
    /// A semantic assertion with explicit expected criteria.
    #[default]
    Assertive,
    /// A lightweight health/sanity check that may lack explicit criteria.
    Smoke,
    /// An observational probe whose result is recorded but not asserted.
    Informational,
}
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
    /// Classification of the scenario's intent.
    pub classification: ScenarioClassification,
}
```

```rust
impl ScenarioMeta {
    pub fn criteria_summary (&self) -> Option<String>;
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
pub struct ScenarioSubResult {
    /// Identifier for the sub-probe.
    pub sub_id: String,
    /// Classification of this sub-result.
    pub classification: ScenarioClassification,
    /// Whether the sub-probe passed.
    pub passed: bool,
    /// Human-readable criteria checked by the sub-probe.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub criteria: Option<String>,
    /// Short excerpt or hash of the response evaluated.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub response_excerpt: Option<String>,
    /// Identifiers of any violations detected.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub violation_ids: Vec<String>,
}
```

```rust
pub struct ScenarioRunOutcome {
    /// Overall pass/fail result of the scenario.
    pub result: Result<()>,
    /// Optional structured sub-results for multi-probe scenarios.
    pub sub_results: Vec<ScenarioSubResult>,
}
```

```rust
impl ScenarioRunOutcome {
    pub fn pass () -> Self;
    pub fn fail (error: Error) -> Self;
    pub fn with_sub_results (mut self, sub_results: Vec<ScenarioSubResult>) -> Self;
}
```

```rust
pub struct ScenarioResult {
    /// Metadata describing the scenario.
    pub meta: ScenarioMeta,
    /// Outcome of the run.
    pub outcome: ScenarioOutcome,
    /// Structured sub-results, when produced by multi-probe scenarios.
    pub sub_results: Vec<ScenarioSubResult>,
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
