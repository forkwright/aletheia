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
//! The runner is network-bound and can take hours against a real benchmark
//! dataset. See [`BenchmarkRunnerConfig`] for per-question timeouts and
//! concurrency controls.

use std::time::Duration;

use tracing::{info, instrument, warn};

use crate::client::EvalClient;
use crate::error::{BenchmarkSnafu, Result};

use super::{
    BenchmarkQuestion, BenchmarkReport, MemoryBenchmark, QuestionResult, judge, metrics,
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
        }
    }
}

/// Runs a memory benchmark against a live aletheia instance.
pub struct BenchmarkRunner {
    client: EvalClient,
    config: BenchmarkRunnerConfig,
}

impl BenchmarkRunner {
    /// Create a new runner with the given client and configuration.
    #[must_use]
    pub fn new(client: EvalClient, config: BenchmarkRunnerConfig) -> Self {
        Self { client, config }
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

        let report = BenchmarkReport::new(benchmark.name(), results);
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
        let session_key = format!("{}-{}-{index}", self.config.session_key_prefix, question.id);

        let answer = match tokio::time::timeout(
            self.config.question_timeout,
            self.ingest_and_ask(&question, &session_key),
        )
        .await
        {
            Ok(Ok(answer)) => answer,
            Ok(Err(e)) => {
                warn!(
                    id = %question.id,
                    error = %e,
                    "benchmark question failed — scoring as no-answer"
                );
                String::new()
            }
            Err(_) => {
                let e = crate::error::TimeoutSnafu {
                    elapsed_ms: millis_from_duration(self.config.question_timeout),
                }
                .build();
                warn!(
                    id = %question.id,
                    error = %e,
                    "benchmark question timed out — scoring as no-answer"
                );
                String::new()
            }
        };

        let score = score_answer(&answer, &question.expected_answers);

        let judge_score = if let Some(ref config) = self.config.judge {
            let judge = judge::LlmJudge::new(config.clone());
            let expected = question.expected_answers.first().map_or("", String::as_str);
            match judge.judge(&question.question, &answer, expected).await {
                Ok(js) => Some(js),
                Err(e) => {
                    warn!(id = %question.id, error = %e, "judge evaluation failed");
                    None
                }
            }
        } else {
            None
        };

        let (retrieved_facts, recall_at_k, ndcg_at_k) = if let Some(k) = self.config.retrieval_k {
            match self
                .client
                .search_knowledge(
                    &question.question,
                    &self.config.nous_id,
                    u32::try_from(k).unwrap_or(u32::MAX),
                )
                .await
            {
                Ok(resp) => {
                    let facts: Vec<String> = resp.facts.into_iter().map(|f| f.content).collect();
                    let r = metrics::recall_at_k(&facts, &question.expected_answers, k);
                    let n = metrics::ndcg_at_k(&facts, &question.expected_answers, k);
                    (Some(facts), Some(r), Some(n))
                }
                Err(e) => {
                    warn!(id = %question.id, error = %e, "knowledge search failed");
                    (None, None, None)
                }
            }
        } else {
            (None, None, None)
        };

        QuestionResult {
            id: question.id,
            category: question.category,
            actual_answer: answer,
            expected_answers: question.expected_answers,
            score,
            judge_score,
            retrieved_facts,
            recall_at_k,
            ndcg_at_k,
        }
    }

    /// Ingest haystack sessions, ask the question, return the assistant's answer.
    async fn ingest_and_ask(
        &self,
        question: &BenchmarkQuestion,
        session_key: &str,
    ) -> Result<String> {
        let session = self
            .client
            .create_session(&self.config.nous_id, session_key)
            .await?;
        let session_id = session.id;

        // WHY: we only replay user turns — the assistant's historical responses
        // would contaminate the answer signal. The memory pipeline sees the
        // user facts and extracts them.
        for haystack in &question.sessions {
            for (role, content) in haystack {
                if !role_is_user(role) {
                    continue;
                }
                if content.trim().is_empty() {
                    continue;
                }
                // WHY: Per-turn errors are best-effort ignored — the assistant may
                // refuse or hit rate limits; the final question must still be asked.
                // kanon:ignore RUST/no-silent-result-swallow — per-turn ingestion errors are intentionally best-effort
                let _ = self.client.send_message(&session_id, content).await;
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
        }

        if answer.trim().is_empty() {
            return Err(BenchmarkSnafu {
                message: format!("empty answer for question {}", question.id),
            }
            .build());
        }

        Ok(answer)
    }
}

/// Return true if the role label corresponds to a user-authored turn.
///
/// Supports both aletheia-native roles (`user`) and benchmark-native speaker
/// labels (e.g. `Alice`, `Bob`). Assistant roles (`assistant`, `Assistant`,
/// `system`) are treated as non-user.
fn role_is_user(role: &str) -> bool {
    let lower = role.to_lowercase();
    lower != "assistant" && lower != "system" && lower != "tool_result" && lower != "tool"
}

fn millis_from_duration(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
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
}
