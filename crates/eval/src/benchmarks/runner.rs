//! Live benchmark runner: executes a [`MemoryBenchmark`] against a running
//! aletheia instance via HTTP.
//!
//! For each question in the dataset:
//! 1. Create a fresh session for the haystack ingestion
//! 2. POST every turn from the haystack sessions as a user message
//!    (so the memory pipeline extracts facts during ingestion)
//! 3. POST the question as a final user message
//! 4. Collect the assistant's SSE response
//! 5. Score the response against the expected answers
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

use std::time::Duration;

use tracing::{info, instrument, warn};

use crate::client::EvalClient;
use crate::error::Result;
use crate::provenance::EvalProvenance;

use super::{
    BenchmarkIngestionLog, BenchmarkIngestionMode, BenchmarkQuestion, BenchmarkReport,
    BenchmarkTurn, MemoryBenchmark, QuestionResult, QuestionStatus, RetrievalScoring,
    RetrievalScoringMode, RetrievedFact, TurnIngestionOutcome, TurnIngestionRecord, judge, metrics,
    score_answer,
};

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
    /// When true, run each question under a disposable per-question memory
    /// namespace derived from `nous_id`, the run id, and the question id.
    /// Session keys and provenance are tagged with `isolated` and the question
    /// id so reports can distinguish official isolated metrics.
    /// When false, all questions reuse the configured `nous_id` (simulates
    /// continuous memory) and session keys are tagged with `continuous`.
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
            provenance: EvalProvenance::new("er-benchmark-default", "http://localhost"),
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
    #[instrument(skip(self, benchmark), fields(benchmark = benchmark.name()))]
    pub async fn run(&self, benchmark: &dyn MemoryBenchmark) -> Result<BenchmarkReport> {
        let total = benchmark.len();
        let limit = self.config.max_questions.unwrap_or(total);
        info!(
            benchmark = benchmark.name(),
            total, limit, "starting benchmark run"
        );

        let mut results = Vec::new();
        for (i, question) in benchmark.questions().take(limit).enumerate() {
            info!(
                index = i + 1,
                id = %question.id,
                category = %question.category,
                "processing benchmark question"
            );

            let result = self.run_question(i, question).await;
            results.push(result);
        }

        let memory_mode = if self.config.close_between_questions {
            "isolated"
        } else {
            "continuous"
        };
        let provenance = self
            .config
            .provenance
            .clone()
            .with_audit_refs(
                None,
                None,
                None,
                None,
                Some(format!(
                    "memory-benchmark-{}-nous-{}",
                    memory_mode, self.config.nous_id
                )),
            )
            .finished();
        let report = BenchmarkReport::new(benchmark.name(), results).with_provenance(provenance);
        info!(
            total = report.total,
            em_rate = report.exact_match_rate(),
            mean_f1 = report.mean_f1(),
            "benchmark run complete"
        );
        Ok(report)
    }

    /// Execute a single question end-to-end: ingest sessions, ask question, score.
    async fn run_question(&self, index: usize, question: BenchmarkQuestion) -> QuestionResult {
        let namespace = question_namespace(&self.config, index, &question.id);
        info!(
            index = index + 1,
            id = %question.id,
            category = %question.category,
            memory_mode = ?namespace.mode,
            namespace = %namespace.tag,
            "processing benchmark question"
        );

        let execution = self.execute_question(&question, &namespace).await;
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
        namespace: &QuestionNamespace,
    ) -> QuestionExecution {
        match tokio::time::timeout(
            self.config.question_timeout,
            self.ingest_and_ask(question, namespace),
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
                            error = ?score.error_message,
                            "judge evaluation failed"
                        );
                    }
                    Some(score)
                }
                Err(e) => {
                    warn!(id = %question.id, error = %e, "judge evaluation failed");
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
                    warn!(id = %question.id, error = %e, "knowledge search failed");
                    (None, None, None, None)
                }
            }
        } else {
            (None, None, None, None)
        }
    }

    /// Ingest haystack sessions, ask the question, return the assistant's answer
    /// and a per-turn ingestion log.
    async fn ingest_and_ask(
        &self,
        question: &BenchmarkQuestion,
        namespace: &QuestionNamespace,
    ) -> Result<(String, BenchmarkIngestionLog)> {
        let session = self
            .client
            .create_session(&namespace.effective_nous_id, &namespace.session_key)
            .await?;
        let session_id = session.id;

        // WHY: Best-effort verification that the backend bound the session to
        // the expected namespace. A mismatch means the isolation contract is
        // broken (e.g., the backend rejected the derived `nous_id` and fell
        // back to the configured one). This is a warning, not a hard failure,
        // because some backends may not support disposable per-question agents.
        if session.nous_id != namespace.effective_nous_id {
            warn!(
                id = %question.id,
                expected_nous_id = %namespace.effective_nous_id,
                actual_nous_id = %session.nous_id,
                "session created under unexpected nous_id; memory isolation may be compromised"
            );
        }

        let mut log = BenchmarkIngestionLog {
            mode: self.ingestion_mode,
            ..BenchmarkIngestionLog::default()
        };

        // WHY: User-only mode replays user turns as plain user messages. In
        // role-preserving mode every non-empty turn is sent as a user message
        // with a structured provenance header so original transcript roles are
        // preserved. Assistant/system/tool turns are tagged as transcript
        // evidence and are not treated as fresh assistant answers.
        for haystack in &question.sessions {
            for turn in haystack {
                if turn.content.trim().is_empty() {
                    log.turns.push(TurnIngestionRecord {
                        role: turn.role.clone(),
                        outcome: TurnIngestionOutcome::Excluded,
                        error_message: Some("empty content".to_owned()),
                        provenance: turn.provenance.clone(),
                    });
                    log.excluded_count += 1;
                    continue;
                }

                let is_user = role_is_user(&turn.role);
                if !is_user && self.ingestion_mode == BenchmarkIngestionMode::UserOnly {
                    log.turns.push(TurnIngestionRecord {
                        role: turn.role.clone(),
                        outcome: TurnIngestionOutcome::Excluded,
                        error_message: None,
                        provenance: turn.provenance.clone(),
                    });
                    log.excluded_count += 1;
                    continue;
                }

                let message = match self.ingestion_mode {
                    BenchmarkIngestionMode::UserOnly => turn.content.clone(),
                    BenchmarkIngestionMode::RolePreserving => format_role_preserving_turn(turn),
                };

                match self.client.send_message(&session_id, &message).await {
                    Ok(_) => {
                        log.turns.push(TurnIngestionRecord {
                            role: turn.role.clone(),
                            outcome: TurnIngestionOutcome::Ingested,
                            error_message: None,
                            provenance: turn.provenance.clone(),
                        });
                        log.ingested_count += 1;
                    }
                    Err(e) => {
                        warn!(
                            id = %question.id,
                            role = %turn.role,
                            error = %e,
                            "benchmark turn ingestion failed; continuing with remaining turns"
                        );
                        log.turns.push(TurnIngestionRecord {
                            role: turn.role.clone(),
                            outcome: TurnIngestionOutcome::Error,
                            error_message: Some(e.to_string()),
                            provenance: turn.provenance.clone(),
                        });
                        log.error_count += 1;
                    }
                }
            }
        }

        let events = self
            .client
            .send_message(&session_id, &question.question)
            .await?;

        let answer = crate::sse::extract_text(&events);

        if self.config.close_between_questions {
            // kanon:ignore RUST/no-silent-result-swallow — session close is best-effort cleanup between benchmark questions
            let _ = self.client.close_session(&session_id).await;
            info!(
                id = %question.id,
                session_id = %session_id,
                namespace = %namespace.tag,
                "closed isolated benchmark session"
            );
        }

        Ok((answer, log))
    }
}

/// Format a benchmark turn for role-preserving ingestion.
///
/// The content is wrapped in a structured provenance header so the original
/// transcript role, speaker, turn id, timestamp, and dataset provenance are
/// retained. The turn is sent as a user message, so historical
/// assistant/system/tool content is not mistaken for a fresh assistant answer.
fn format_role_preserving_turn(turn: &BenchmarkTurn) -> String {
    let speaker = turn.speaker.as_deref().unwrap_or("");
    let turn_id = turn.turn_id.as_deref().unwrap_or("");
    let timestamp = turn.timestamp.as_deref().unwrap_or("");
    let provenance = turn.provenance.as_deref().unwrap_or("");
    format!(
        "[transcript role={} speaker={} turn_id={} timestamp={} provenance={}]\n{}",
        turn.role, speaker, turn_id, timestamp, provenance, turn.content
    )
}

/// Return true if the role label corresponds to a user-authored turn.
///
/// Supports both aletheia-native roles (`user`) and benchmark-native speaker
/// labels (e.g. `Alice`, `Bob`). Assistant roles (`assistant`, `Assistant`,
/// `system`, `tool`, `tool_result`) are treated as non-user.
///
/// Non-user turns are skipped in user-only ingestion mode and sent with a
/// structured provenance header in role-preserving mode.
fn role_is_user(role: &str) -> bool {
    let lower = role.to_lowercase();
    lower != "assistant" && lower != "system" && lower != "tool_result" && lower != "tool"
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
    fn role_is_user_accepts_user() {
        assert!(role_is_user("user"));
        assert!(role_is_user("User"));
        assert!(role_is_user("USER"));
    }

    #[test]
    fn role_is_user_rejects_assistant() {
        assert!(!role_is_user("assistant"));
        assert!(!role_is_user("Assistant"));
        assert!(!role_is_user("system"));
        assert!(!role_is_user("tool_result"));
        assert!(!role_is_user("tool"));
    }

    #[test]
    fn role_is_user_accepts_locomo_speaker_labels() {
        // NOTE: LoCoMo uses speaker labels instead of roles.
        assert!(role_is_user("Alice"));
        assert!(role_is_user("Bob"));
        assert!(role_is_user("Charlie"));
    }

    #[test]
    fn role_preserving_turn_includes_provenance_header() {
        let turn = BenchmarkTurn {
            role: "assistant".to_owned(),
            content: "Your favorite color is blue.".to_owned(),
            speaker: None,
            turn_id: Some("s0:t1".to_owned()),
            timestamp: Some("2024-01-01T00:00:00Z".to_owned()),
            provenance: Some("LongMemEval:q1:session_0:turn_1".to_owned()),
        };

        let formatted = format_role_preserving_turn(&turn);

        assert!(formatted.starts_with("[transcript "));
        assert!(formatted.contains("role=assistant"));
        assert!(formatted.contains("turn_id=s0:t1"));
        assert!(formatted.contains("provenance=LongMemEval:q1:session_0:turn_1"));
        assert!(formatted.ends_with("Your favorite color is blue."));
    }

    #[test]
    fn role_preserving_turn_omits_empty_optional_fields() {
        let turn = BenchmarkTurn {
            role: "user".to_owned(),
            content: "Hello.".to_owned(),
            speaker: None,
            turn_id: None,
            timestamp: None,
            provenance: None,
        };

        let formatted = format_role_preserving_turn(&turn);

        assert_eq!(
            formatted,
            "[transcript role=user speaker= turn_id= timestamp= provenance=]\nHello."
        );
    }

    #[test]
    fn config_default_has_sensible_values() {
        let config = BenchmarkRunnerConfig::default();
        assert_eq!(config.nous_id, "benchmark");
        assert_eq!(config.session_key_prefix, "bench");
        assert_eq!(config.question_timeout, Duration::from_mins(2));
        assert!(config.max_questions.is_none());
        assert!(config.close_between_questions);
        assert!(config.judge.is_none());
        assert!(config.retrieval_k.is_none());
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
