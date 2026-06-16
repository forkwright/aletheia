//! Live benchmark runner: executes a [`MemoryBenchmark`] against a running
//! aletheia instance via HTTP.
//!
//! For each question in the dataset:
//! 1. Create a fresh session for the question (official-parity mode) or reuse
//!    the single continuous-memory session.
//! 2. Seed the full haystack transcript — preserving historical `assistant`,
//!    `system`, `tool`, and speaker-labeled turns — into the knowledge store
//!    through `POST /api/v1/knowledge/ingest`. This keeps the evidence out of
//!    the final prompt as user messages, avoiding answer contamination while
//!    still giving the memory pipeline access to the full conversation.
//! 3. Surface any ingestion errors instead of swallowing them.
//! 4. POST the benchmark question as a user message.
//! 5. Collect the assistant's SSE response and score it.
//!
//! In isolated mode (`close_between_questions == true`) each question runs
//! under a disposable memory namespace derived from the configured `nous_id`,
//! the benchmark run id, and the question id. This prevents typed memory,
//! vector, and fact side effects from leaking across questions. In continuous
//! mode the configured `nous_id` is reused to simulate a long-lived memory.
//!
//! The runner is network-bound and can take hours against a real benchmark
//! dataset. See [`BenchmarkRunnerConfig`] for per-question timeouts and
//! concurrency controls.

use std::fmt::Write as _;
use std::time::Duration;

use tracing::{info, instrument, warn};

use crate::client::EvalClient;
use crate::error::Result;
use crate::provenance::{EvalProvenance, generate_eval_run_id};

use super::{
    BenchmarkIngestionLog, BenchmarkIngestionMode, BenchmarkQuestion, BenchmarkReport,
    BenchmarkTurn, MemoryBenchmark, QuestionResult, QuestionStatus, RetrievalScoring,
    RetrievalScoringMode, RetrievedFact, TurnIngestionOutcome, TurnIngestionRecord, judge, metrics,
    score_answer,
};

/// Benchmark execution mode.
///
/// * `OfficialParity` — each question gets its own session. The session is
///   closed after the question so that prior question/answer pairs do not sit
///   in the live context. This matches the standard evaluation protocol. Full
///   memory isolation also requires a dedicated, disposable `nous_id` for the
///   run; the runner tags every artifact with `eval_run_id` and `question_id`
///   so results can be traced back to a clean namespace.
/// * `ContinuousMemory` — all questions share one long-running session. This
///   simulates a real user conversation where earlier questions and answers
///   remain in context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchmarkMode {
    /// Clean-session protocol used for official results.
    OfficialParity,
    /// Shared-session protocol that simulates continuous use.
    ContinuousMemory,
}

/// Configuration for a benchmark run.
#[derive(Debug, Clone)]
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
    /// When true, each question gets a fresh session (`OfficialParity`).
    /// When false, all questions share one session (`ContinuousMemory`).
    pub close_between_questions: bool,
    /// Optional LLM-as-judge configuration. When set, each answer is also
    /// evaluated by an external LLM for binary correctness.
    pub judge: Option<judge::LlmJudgeConfig>,
    /// When set, query the knowledge store after ingestion and compute
    /// Recall@k and NDCG@k against the expected answers.
    pub retrieval_k: Option<usize>,
    /// Shared provenance envelope for the benchmark run. The
    /// `provenance.eval_run_id` is used to tag generated artifacts.
    pub provenance: EvalProvenance,
}

impl BenchmarkRunnerConfig {
    /// Return the execution mode implied by this configuration.
    #[must_use]
    pub fn mode(&self) -> BenchmarkMode {
        if self.close_between_questions {
            BenchmarkMode::OfficialParity
        } else {
            BenchmarkMode::ContinuousMemory
        }
    }
}

impl Default for BenchmarkRunnerConfig {
    fn default() -> Self {
        Self {
            nous_id: "benchmark".to_owned(),
            session_key_prefix: "bench".to_owned(),
            question_timeout: Duration::from_mins(2),
            max_questions: None,
            close_between_questions: true,
            judge: None,
            retrieval_k: None,
            provenance: EvalProvenance::new(generate_eval_run_id(), "http://localhost"),
        }
    }
}

/// Runs a memory benchmark against a live aletheia instance.
pub struct BenchmarkRunner {
    client: EvalClient,
    config: BenchmarkRunnerConfig,
    ingestion_mode: BenchmarkIngestionMode,
}

struct QuestionExecution {
    answer: String,
    status: QuestionStatus,
    error_message: Option<String>,
    ingestion_log: Option<BenchmarkIngestionLog>,
}

type RetrievalMetrics = (
    Option<Vec<RetrievedFact>>,
    Option<RetrievalScoring>,
    Option<f64>,
    Option<f64>,
);

/// Memory isolation mode for a single benchmark question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemoryIsolationMode {
    /// Disposable per-question namespace: no memory is shared with other
    /// questions.
    Isolated,
    /// Continuous memory: the configured `nous_id` is reused.
    Continuous,
}

/// Per-question identity used to isolate (or intentionally share) memory.
struct QuestionNamespace {
    /// Effective `nous_id` used for session creation and knowledge search.
    effective_nous_id: String,
    /// Unique session key for this question.
    session_key: String,
    /// Isolation mode for this question.
    mode: MemoryIsolationMode,
    /// Human-readable provenance/log tag, e.g. `benchmark/isolated/er-1/q1`.
    tag: String,
}

impl BenchmarkRunner {
    /// Create a new runner with the given client and configuration.
    #[must_use]
    pub fn new(client: EvalClient, config: BenchmarkRunnerConfig) -> Self {
        Self {
            client,
            config,
            ingestion_mode: BenchmarkIngestionMode::UserOnly,
        }
    }

    /// Set the ingestion mode for this runner.
    ///
    /// The default is [`BenchmarkIngestionMode::UserOnly`]. Use
    /// [`BenchmarkIngestionMode::RolePreserving`] when the benchmark contains
    /// assistant/system/tool evidence that must be recalled (e.g. LongMemEval
    /// `single-session-assistant` questions).
    #[must_use]
    pub fn with_ingestion_mode(mut self, mode: BenchmarkIngestionMode) -> Self {
        self.ingestion_mode = mode;
        self
    }

    /// Run a benchmark and return the aggregate report.
    ///
    /// # Errors
    ///
    /// Returns an error if session creation fails for the first question.
    /// Per-question errors after that are recorded as zero-score results
    /// rather than aborting the run.
    #[instrument(skip(self, benchmark), fields(benchmark = benchmark.name(), eval_run_id = %self.config.provenance.eval_run_id))]
    pub async fn run(&self, benchmark: &dyn MemoryBenchmark) -> Result<BenchmarkReport> {
        let total = benchmark.len();
        let limit = self.config.max_questions.unwrap_or(total);
        info!(
            benchmark = benchmark.name(),
            eval_run_id = %self.config.provenance.eval_run_id,
            total, limit,
            mode = ?self.config.mode(),
            "starting benchmark run"
        );

        let mut results = Vec::new();
        let mut shared_session: Option<String> = None;
        for (i, question) in benchmark.questions().take(limit).enumerate() {
            info!(
                index = i + 1,
                id = %question.id,
                category = %question.category,
                eval_run_id = %self.config.provenance.eval_run_id,
                "processing benchmark question"
            );

            let result = self.run_question(i, question, &mut shared_session).await;
            results.push(result);
        }

        // Clean up the shared continuous-memory session at the end of the run.
        if let Some(session_id) = shared_session.take() {
            let _ = self.client.close_session(&session_id).await;
        }

        let report = BenchmarkReport::new(benchmark.name(), results)
            .with_provenance(self.config.provenance.clone().finished());
        info!(
            total = report.total,
            em_rate = report.exact_match_rate(),
            mean_f1 = report.mean_f1(),
            "benchmark run complete"
        );
        Ok(report)
    }

    /// Execute a single question end-to-end: ingest sessions, ask question, score.
    async fn run_question(
        &self,
        index: usize,
        question: BenchmarkQuestion,
        shared_session: &mut Option<String>,
    ) -> QuestionResult {
        let session_key = format!(
            "{}-{}-{index}-{}",
            self.config.session_key_prefix, self.config.provenance.eval_run_id, question.id
        );

        let execution = self
            .execute_question(&question, &session_key, shared_session)
            .await;
        let status = execution.status;
        let score = score_for_status(status, &execution.answer, &question.expected_answers);
        let judge_score = self
            .evaluate_judge(&question, &execution.answer, status)
            .await;
        let (retrieved_facts, retrieval_scoring, recall_at_k, ndcg_at_k) =
            self.evaluate_retrieval(&question, &namespace, status).await;

        QuestionResult {
            id: question.id,
            category: question.category,
            status,
            error_message: execution.error_message,
            actual_answer: execution.answer,
            expected_answers: question.expected_answers,
            expected_evidence_refs: question.expected_evidence_refs,
            score,
            judge_score,
            retrieved_facts,
            retrieval_scoring,
            recall_at_k,
            ndcg_at_k,
            ingestion_log: execution.ingestion_log,
        }
    }

    async fn execute_question(
        &self,
        question: &BenchmarkQuestion,
        session_key: &str,
        shared_session: &mut Option<String>,
    ) -> QuestionExecution {
        match tokio::time::timeout(
            self.config.question_timeout,
            self.ingest_and_ask(question, session_key, shared_session),
        )
        .await
        {
            Ok(Ok((answer, log))) if answer.trim().is_empty() => QuestionExecution {
                answer,
                status: QuestionStatus::NoAnswer,
                error_message: Some("empty answer".to_owned()),
                ingestion_log: Some(log),
            },
            Ok(Ok((answer, log))) => QuestionExecution {
                answer,
                status: QuestionStatus::Scored,
                error_message: None,
                ingestion_log: Some(log),
            },
            Ok(Err(e)) => {
                warn!(
                    id = %question.id,
                    eval_run_id = %self.config.provenance.eval_run_id,
                    error = %e,
                    "benchmark question failed before producing a scorable answer"
                );
                QuestionExecution {
                    answer: String::new(),
                    status: QuestionStatus::Error,
                    error_message: Some(e.to_string()),
                    ingestion_log: None,
                }
            }
            Err(_) => {
                let e = crate::error::TimeoutSnafu {
                    elapsed_ms: millis_from_duration(self.config.question_timeout),
                }
                .build();
                warn!(
                    id = %question.id,
                    eval_run_id = %self.config.provenance.eval_run_id,
                    error = %e,
                    "benchmark question timed out before producing a scorable answer"
                );
                QuestionExecution {
                    answer: String::new(),
                    status: QuestionStatus::Timeout,
                    error_message: Some(e.to_string()),
                    ingestion_log: None,
                }
            }
        }
    }

    async fn evaluate_judge(
        &self,
        question: &BenchmarkQuestion,
        answer: &str,
        status: QuestionStatus,
    ) -> Option<judge::JudgeScore> {
        if status.is_scored()
            && let Some(ref config) = self.config.judge
        {
            match judge::LlmJudge::new(config.clone()) {
                Ok(judge) => {
                    let score = judge
                        .judge(&question.question, answer, &question.expected_answers)
                        .await;
                    if !score.status.is_scored() {
                        warn!(
                            id = %question.id,
                            eval_run_id = %self.config.provenance.eval_run_id,
                            error = ?score.error_message,
                            "judge evaluation failed"
                        );
                    }
                    Some(score)
                }
                Err(e) => {
                    warn!(
                        id = %question.id,
                        eval_run_id = %self.config.provenance.eval_run_id,
                        error = %e,
                        "judge evaluation failed"
                    );
                    Some(judge::JudgeScore::configuration_error(
                        config,
                        &question.question,
                        answer,
                        &question.expected_answers,
                        e.to_string(),
                    ))
                }
            }
        } else {
            None
        }
    }

    async fn evaluate_retrieval(
        &self,
        question: &BenchmarkQuestion,
        namespace: &QuestionNamespace,
        status: QuestionStatus,
    ) -> RetrievalMetrics {
        if status.is_scored()
            && let Some(k) = self.config.retrieval_k
        {
            match self
                .client
                .search_knowledge(
                    &question.question,
                    &namespace.effective_nous_id,
                    u32::try_from(k).unwrap_or(u32::MAX),
                )
                .await
            {
                Ok(resp) => {
                    let mut retrieved = Vec::new();
                    let mut retrieved_evidence_refs = Vec::new();
                    let mut retrieved_content_refs = Vec::new();
                    for fact in resp.facts {
                        let id = if fact.id.trim().is_empty() {
                            None
                        } else {
                            Some(fact.id.clone())
                        };
                        if let Some(ref id) = id {
                            retrieved_evidence_refs.push(normalize_evidence_ref(id));
                        }
                        let content_sha256 = crate::provenance::sha256_hex_str(&fact.content);
                        retrieved_content_refs.push(metrics::normalized_content_ref(&fact.content));
                        let reference = id.as_ref().map_or_else(
                            || format!("content_sha256:{content_sha256}"),
                            |id| format!("fact:{id}"),
                        );
                        retrieved.push(RetrievedFact {
                            id,
                            reference,
                            score: fact.score,
                            confidence: fact.confidence,
                            content_sha256,
                        });
                    }
                    let (scoring, retrieved_refs) = retrieval_scoring_refs(
                        question,
                        retrieved_evidence_refs,
                        retrieved_content_refs,
                    );
                    let r = metrics::recall_at_k(&retrieved_refs, &scoring.relevant_refs, k);
                    let n = metrics::ndcg_at_k(&retrieved_refs, &scoring.relevant_refs, k);
                    (Some(retrieved), Some(scoring), Some(r), Some(n))
                }
                Err(e) => {
                    warn!(
                        id = %question.id,
                        eval_run_id = %self.config.provenance.eval_run_id,
                        error = %e,
                        "knowledge search failed"
                    );
                    (None, None, None, None)
                }
            }
        } else {
            (None, None, None, None)
        }
    }

    /// Ingest the haystack transcript into the knowledge store, ask the
    /// question, and return the assistant's answer.
    async fn ingest_and_ask(
        &self,
        question: &BenchmarkQuestion,
        session_key: &str,
        shared_session: &mut Option<String>,
    ) -> Result<String> {
        // Create a new session for official parity, or reuse the single
        // continuous-memory session.
        let session_id = if let (BenchmarkMode::ContinuousMemory, Some(id)) =
            (self.config.mode(), shared_session.as_ref())
        {
            id.clone()
        } else {
            let session = self
                .client
                .create_session(&self.config.nous_id, session_key)
                .await?;
            let id = session.id;
            if self.config.mode() == BenchmarkMode::ContinuousMemory {
                *shared_session = Some(id.clone());
            }
            id
        };

        // WHY: Seed the full transcript into the knowledge store instead of
        // replaying every retained turn as a user message. This preserves
        // historical assistant/system/tool/speaker evidence while keeping the
        // final answer turn free of contamination from the haystack.
        let transcript = build_transcript_markdown(question);
        if !transcript.trim().is_empty() {
            let response = self
                .client
                .ingest_transcript(&self.config.nous_id, &transcript)
                .await?;

            if !response.errors.is_empty() {
                let summary = response
                    .errors
                    .iter()
                    .take(5)
                    .map(|e| {
                        let id = e.id.as_deref().unwrap_or("-");
                        format!("fact[{}] id={id}: {}", e.index, e.message)
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                return crate::error::BenchmarkSnafu {
                    message: format!(
                        "transcript ingestion failed ({} errors, {} inserted, {} skipped): {summary}",
                        response.errors.len(),
                        response.inserted,
                        response.skipped
                    ),
                }
                .fail();
            }
        }

        let events = self
            .client
            .send_message(&session_id, &question.question)
            .await?;

        let answer = crate::sse::extract_text(&events);

        if self.config.mode() == BenchmarkMode::OfficialParity {
            // WHY: Best-effort cleanup keeps the live session list from
            // growing unbounded during a large benchmark run.
            let _ = self.client.close_session(&session_id).await;
            if shared_session.as_ref() == Some(&session_id) {
                *shared_session = None;
            }
        }

        Ok((answer, log))
    }
}

/// Build a markdown document that preserves every turn and its original role.
///
/// The document is sent to `POST /api/v1/knowledge/ingest` so the memory
/// pipeline can extract facts from the full conversation without the turns
/// being replayed as user messages.
fn build_transcript_markdown(question: &BenchmarkQuestion) -> String {
    let mut out = String::new();
    for (session_idx, session) in question.sessions.iter().enumerate() {
        let _ = write!(out, "## Session {session_idx}\n\n");
        for (turn_idx, (role, content)) in session.iter().enumerate() {
            if content.trim().is_empty() {
                continue;
            }
            let _ = write!(out, "### turn {turn_idx} — {role}\n\n{content}\n\n");
        }
    }
    out
}

fn millis_from_duration(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn score_for_status(
    status: QuestionStatus,
    answer: &str,
    expected_answers: &[String],
) -> metrics::BenchmarkScore {
    if status.is_scored() {
        score_answer(answer, expected_answers)
    } else {
        metrics::BenchmarkScore::zero()
    }
}

fn retrieval_scoring_refs(
    question: &BenchmarkQuestion,
    retrieved_evidence_refs: Vec<String>,
    retrieved_content_refs: Vec<String>,
) -> (RetrievalScoring, Vec<String>) {
    let expected_evidence_refs: Vec<String> = question
        .expected_evidence_refs
        .iter()
        .map(|reference| normalize_evidence_ref(reference))
        .filter(|reference| !reference.is_empty())
        .collect();
    if expected_evidence_refs.is_empty() {
        let relevant_refs: Vec<String> = question
            .expected_answers
            .iter()
            .map(|answer| metrics::normalized_content_ref(answer))
            .collect();
        (
            RetrievalScoring {
                mode: RetrievalScoringMode::NormalizedContent,
                fallback_used: true,
                relevant_refs,
            },
            retrieved_content_refs,
        )
    } else {
        (
            RetrievalScoring {
                mode: RetrievalScoringMode::EvidenceId,
                fallback_used: false,
                relevant_refs: expected_evidence_refs,
            },
            retrieved_evidence_refs,
        )
    }
}

fn normalize_evidence_ref(reference: &str) -> String {
    let trimmed = reference.trim();
    trimmed
        .strip_prefix("fact:")
        .unwrap_or(trimmed)
        .trim()
        .to_owned()
}

/// Build the per-question identity used for session creation and knowledge
/// search.
///
/// In isolated mode the effective `nous_id` is derived from the configured
/// `nous_id`, the benchmark run id, and the question id so that typed memory,
/// vector, and fact side effects cannot leak across questions. The session key
/// and namespace tag include the run id, question id, and mode so that reports
/// and logs can distinguish official isolated runs from continuous-memory
/// experiments.
///
/// In continuous mode the configured `nous_id` is preserved and the session key
/// is tagged with `continuous`.
fn question_namespace(
    config: &BenchmarkRunnerConfig,
    index: usize,
    question_id: &str,
) -> QuestionNamespace {
    let run_id = sanitize_id_part(&config.provenance.eval_run_id);
    let sanitized_question_id = sanitize_id_part(question_id);

    if config.close_between_questions {
        let effective_nous_id = format!("{}-{}-{}", config.nous_id, run_id, sanitized_question_id);
        let session_key = format!(
            "{}-{}-{}-{}-isolated",
            config.session_key_prefix, run_id, sanitized_question_id, index
        );
        let tag = format!(
            "{}/isolated/{}/{}",
            config.nous_id, run_id, sanitized_question_id
        );
        QuestionNamespace {
            effective_nous_id,
            session_key,
            mode: MemoryIsolationMode::Isolated,
            tag,
        }
    } else {
        let session_key = format!(
            "{}-{}-{}-{}-continuous",
            config.session_key_prefix, run_id, sanitized_question_id, index
        );
        let tag = format!("{}/continuous", config.nous_id);
        QuestionNamespace {
            effective_nous_id: config.nous_id.clone(),
            session_key,
            mode: MemoryIsolationMode::Continuous,
            tag,
        }
    }
}

/// Sanitize an arbitrary string so it can be embedded in a `nous_id`-compatible
/// identifier.
///
/// The backend `AgentDefinition` only accepts ASCII alphanumeric characters and
/// hyphens, with no leading or trailing hyphen. This helper collapses any other
/// characters into single hyphens and lowercases the result.
fn sanitize_id_part(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_hyphen = true; // treat start as hyphen to avoid a leading hyphen
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_hyphen = false;
        } else if !prev_hyphen {
            out.push('-');
            prev_hyphen = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push('x');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]

    fn config_default_has_sensible_values() {
        let config = BenchmarkRunnerConfig::default();
        assert_eq!(config.nous_id, "benchmark");
        assert_eq!(config.session_key_prefix, "bench");
        assert_eq!(config.question_timeout, Duration::from_mins(2));
        assert!(config.max_questions.is_none());
        assert!(config.close_between_questions);
        assert_eq!(config.mode(), BenchmarkMode::OfficialParity);
        assert!(!config.provenance.eval_run_id.is_empty());
        assert!(config.judge.is_none());
        assert!(config.retrieval_k.is_none());
    }

    #[test]
    fn continuous_mode_derived_from_close_between_questions_false() {
        let config = BenchmarkRunnerConfig {
            close_between_questions: false,
            ..Default::default()
        };
        assert_eq!(config.mode(), BenchmarkMode::ContinuousMemory);
    }

    #[test]
    fn config_max_questions_limits_iteration() {
        let config = BenchmarkRunnerConfig {
            max_questions: Some(3),
            ..Default::default()
        };
        // NOTE: Simulates the `.take(limit)` pattern the runner uses.
        let items: Vec<i32> = (0..10).collect();
        let taken: Vec<_> = items
            .iter()
            .take(config.max_questions.unwrap_or(items.len()))
            .collect();
        assert_eq!(taken.len(), 3);
    }

    #[test]
    fn transcript_markdown_preserves_roles() {
        let question = BenchmarkQuestion {
            id: "q1".to_owned(),
            sessions: vec![vec![
                ("user".to_owned(), "My favorite color is blue.".to_owned()),
                ("assistant".to_owned(), "Noted.".to_owned()),
                ("system".to_owned(), "Preference updated.".to_owned()),
            ]],
            question: "What color?".to_owned(),
            expected_answers: vec!["blue".to_owned()],
            expected_evidence_refs: Vec::new(),
            category: "single-session".to_owned(),
        };

        let md = build_transcript_markdown(&question);
        assert!(md.contains("## Session 0"));
        assert!(md.contains("### turn 0 — user"));
        assert!(md.contains("My favorite color is blue."));
        assert!(md.contains("### turn 1 — assistant"));
        assert!(md.contains("Noted."));
        assert!(md.contains("### turn 2 — system"));
        assert!(md.contains("Preference updated."));
    }

    #[test]
    fn transcript_markdown_skips_empty_turns() {
        let question = BenchmarkQuestion {
            id: "q1".to_owned(),
            sessions: vec![vec![
                ("user".to_owned(), "Hello".to_owned()),
                ("assistant".to_owned(), "   ".to_owned()),
                ("user".to_owned(), "World".to_owned()),
            ]],
            question: "What?".to_owned(),
            expected_answers: vec!["World".to_owned()],
            expected_evidence_refs: Vec::new(),
            category: "single-session".to_owned(),
        };

        let md = build_transcript_markdown(&question);
        assert!(md.contains("Hello"));
        assert!(!md.contains("turn 1"));
        assert!(md.contains("World"));
    }

    #[test]
    fn retrieval_scoring_prefers_evidence_ids() {
        let question = BenchmarkQuestion {
            id: "q1".to_owned(),
            sessions: Vec::new(),
            question: "What color?".to_owned(),
            expected_answers: vec!["blue".to_owned()],
            expected_evidence_refs: vec!["fact:fact-blue".to_owned()],
            category: "single-session-user".to_owned(),
        };

        let (scoring, refs) = retrieval_scoring_refs(
            &question,
            vec!["fact-blue".to_owned()],
            vec![metrics::normalized_content_ref("not the answer")],
        );

        assert_eq!(scoring.mode, RetrievalScoringMode::EvidenceId);
        assert!(!scoring.fallback_used);
        assert_eq!(scoring.relevant_refs, vec!["fact-blue"]);
        assert_eq!(refs, vec!["fact-blue"]);
    }

    #[test]
    fn retrieval_scoring_reports_content_fallback() {
        let question = BenchmarkQuestion {
            id: "q1".to_owned(),
            sessions: Vec::new(),
            question: "What color?".to_owned(),
            expected_answers: vec!["blue".to_owned()],
            expected_evidence_refs: Vec::new(),
            category: "single-session-user".to_owned(),
        };

        let content_ref = metrics::normalized_content_ref("blue");
        let (scoring, refs) =
            retrieval_scoring_refs(&question, Vec::new(), vec![content_ref.clone()]);

        assert_eq!(scoring.mode, RetrievalScoringMode::NormalizedContent);
        assert!(scoring.fallback_used);
        assert_eq!(scoring.relevant_refs, vec![content_ref.clone()]);
        assert_eq!(refs, vec![content_ref]);
    }

    #[test]
    fn isolated_namespace_derives_from_config_run_and_question() {
        let config = BenchmarkRunnerConfig {
            nous_id: "benchmark".to_owned(),
            session_key_prefix: "bench".to_owned(),
            provenance: EvalProvenance::new("er-test-123", "http://localhost"),
            ..Default::default()
        };

        let ns = question_namespace(&config, 7, "sample_42.q");

        assert_eq!(ns.mode, MemoryIsolationMode::Isolated);
        assert_eq!(ns.effective_nous_id, "benchmark-er-test-123-sample-42-q");
        assert!(
            ns.session_key
                .starts_with("bench-er-test-123-sample-42-q-7-isolated"),
            "session_key should be tagged with run id, question id, index, and mode: {}",
            ns.session_key
        );
        assert_eq!(ns.tag, "benchmark/isolated/er-test-123/sample-42-q");
    }

    #[test]
    fn continuous_namespace_preserves_configured_nous_id() {
        let config = BenchmarkRunnerConfig {
            nous_id: "benchmark".to_owned(),
            session_key_prefix: "bench".to_owned(),
            close_between_questions: false,
            provenance: EvalProvenance::new("er-test-456", "http://localhost"),
            ..Default::default()
        };

        let ns = question_namespace(&config, 3, "q1");

        assert_eq!(ns.mode, MemoryIsolationMode::Continuous);
        assert_eq!(ns.effective_nous_id, "benchmark");
        assert!(
            ns.session_key.ends_with("-continuous"),
            "session_key should be tagged continuous: {}",
            ns.session_key
        );
        assert_eq!(ns.tag, "benchmark/continuous");
    }

    #[test]
    fn isolated_namespaces_are_unique_per_question() {
        let config = BenchmarkRunnerConfig {
            provenance: EvalProvenance::new("er-run-abc", "http://localhost"),
            ..Default::default()
        };

        let ns1 = question_namespace(&config, 0, "q1");
        let ns2 = question_namespace(&config, 1, "q2");

        assert_ne!(ns1.effective_nous_id, ns2.effective_nous_id);
        assert_ne!(ns1.session_key, ns2.session_key);
    }

    #[test]
    fn sanitize_id_part_produces_nous_compatible_identifiers() {
        assert_eq!(sanitize_id_part("q1"), "q1");
        assert_eq!(sanitize_id_part("sample_42.q"), "sample-42-q");
        assert_eq!(sanitize_id_part("--weird--"), "weird");
        assert_eq!(sanitize_id_part(""), "x");
        assert_eq!(sanitize_id_part("ALL_CAPS"), "all-caps");
        // Only ASCII alphanumeric and hyphen characters are present.
        let sanitized = sanitize_id_part("a.b_c-d/e");
        assert!(
            sanitized
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        );
        assert!(!sanitized.starts_with('-'));
        assert!(!sanitized.ends_with('-'));
    }
}
