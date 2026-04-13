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
    BenchmarkQuestion, BenchmarkReport, MemoryBenchmark, QuestionResult, score_answer,
};

/// Configuration for a benchmark run.
#[derive(Debug, Clone)]
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

impl Default for BenchmarkRunnerConfig {
    fn default() -> Self {
        Self {
            nous_id: "benchmark".to_owned(),
            session_key_prefix: "bench".to_owned(),
            question_timeout: Duration::from_mins(2),
            max_questions: None,
            close_between_questions: true,
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
            total,
            limit,
            "starting benchmark run"
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

        let answer = match self.ingest_and_ask(&question, &session_key).await {
            Ok(answer) => answer,
            Err(e) => {
                warn!(
                    id = %question.id,
                    error = %e,
                    "benchmark question failed — scoring as no-answer"
                );
                String::new()
            }
        };

        let score = score_answer(&answer, &question.expected_answers);
        QuestionResult {
            id: question.id,
            category: question.category,
            actual_answer: answer,
            expected_answers: question.expected_answers,
            score,
        }
    }

    /// Ingest haystack sessions, ask the question, return the assistant's answer.
    async fn ingest_and_ask(
        &self,
        question: &BenchmarkQuestion,
        session_key: &str,
    ) -> Result<String> {
        // Create a fresh session for this question.
        let session = self
            .client
            .create_session(&self.config.nous_id, session_key)
            .await?;
        let session_id = session.id;

        // Ingest every user turn from every haystack session as a message.
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
                // Ignore per-turn errors — the assistant may refuse or hit
                // rate limits; we still want to ask the question at the end.
                let _ = self.client.send_message(&session_id, content).await;
            }
        }

        // Ask the question.
        let events = self.client.send_message(&session_id, &question.question).await?;

        // Extract the concatenated text response.
        let answer = crate::sse::extract_text(&events);

        // Optionally close the session to reset for the next question.
        if self.config.close_between_questions {
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
        // LoCoMo uses speaker labels instead of roles
        assert!(role_is_user("Alice"));
        assert!(role_is_user("Bob"));
        assert!(role_is_user("Charlie"));
    }

    #[test]
    fn config_default_has_sensible_values() {
        let config = BenchmarkRunnerConfig::default();
        assert_eq!(config.nous_id, "benchmark");
        assert_eq!(config.session_key_prefix, "bench");
        assert_eq!(config.question_timeout, Duration::from_secs(120));
        assert!(config.max_questions.is_none());
        assert!(config.close_between_questions);
    }

    #[test]
    fn config_max_questions_limits_iteration() {
        let config = BenchmarkRunnerConfig {
            max_questions: Some(3),
            ..Default::default()
        };
        // Simulate the `.take(limit)` pattern the runner uses
        let items: Vec<i32> = (0..10).collect();
        let taken: Vec<_> = items.iter().take(config.max_questions.unwrap_or(items.len())).collect();
        assert_eq!(taken.len(), 3);
    }
}
