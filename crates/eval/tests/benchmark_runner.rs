//! End-to-end integration tests for the benchmark runner using a mock server.
//!
//! These tests spin up a wiremock server that impersonates the aletheia API
//! enough for the benchmark runner to exercise the full pipeline: session
//! creation → haystack ingestion → question → SSE response → scoring.

#![allow(clippy::indexing_slicing, reason = "integration test boilerplate")]

use dokimion::benchmarks::longmemeval::LongMemEvalDataset;
use dokimion::benchmarks::{
    BenchmarkRunner, BenchmarkRunnerConfig, EvalClient, QuestionStatus, RetrievalScoringMode,
};
use wiremock::matchers::{method, path, path_regex, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

// kanon:ignore RUST/box-dyn-error — integration test uses Box<dyn Error> for ergonomic test result propagation
type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Initialize the rustls default crypto provider once per test run.
/// Required because the workspace uses `reqwest` with `rustls-no-provider`.
fn init_crypto() {
    // install_default() is idempotent: subsequent calls return Err and are ignored.
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Build an SSE response body that emits a single `text_delta` and a
/// `message_complete` event. This mimics the aletheia streaming format.
// kanon:ignore RUST/doc-promised-observability — doc uses "emits" in SSE protocol sense, not tracing observability
fn sse_response(text: &str) -> String {
    format!(
        "event: text_delta\n\
         data: {{\"type\":\"text_delta\",\"text\":\"{text}\"}}\n\n\
         event: message_complete\n\
         data: {{\"type\":\"message_complete\",\"stop_reason\":\"end_turn\",\"usage\":{{\"input_tokens\":10,\"output_tokens\":20}}}}\n\n"
    )
}

/// Register the mock endpoints needed for a benchmark run.
async fn setup_mock_server(answer_text: &str) -> MockServer {
    let server = MockServer::start().await;

    // POST /api/v1/sessions → 201 with a session ID + all fields SessionResponse requires
    Mock::given(method("POST"))
        .and(path("/api/v1/sessions"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": "sess_benchmark_123",
            "nous_id": "benchmark",
            "session_key": "bench-key",
            "status": "active",
            "model": null,
            "message_count": 0,
            "token_count_estimate": 0,
            "created_at": "2026-04-10T00:00:00Z",
            "updated_at": "2026-04-10T00:00:00Z"
        })))
        .mount(&server)
        .await;

    // POST /api/v1/sessions/{id}/messages → SSE stream
    Mock::given(method("POST"))
        .and(path_regex(r"^/api/v1/sessions/[^/]+/messages$"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse_response(answer_text)),
        )
        .mount(&server)
        .await;

    // DELETE /api/v1/sessions/{id} → 204
    Mock::given(method("DELETE"))
        .and(path_regex(r"^/api/v1/sessions/[^/]+$"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    // POST /api/v1/knowledge/ingest → 200 (seed transcript into memory substrate)
    Mock::given(method("POST"))
        .and(path("/api/v1/knowledge/ingest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "inserted": 1,
            "skipped": 0,
            "errors": []
        })))
        .mount(&server)
        .await;

    server
}

async fn setup_slow_mock_server(answer_text: &str, delay: std::time::Duration) -> MockServer {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/sessions"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": "sess_benchmark_slow",
            "nous_id": "benchmark",
            "session_key": "bench-key",
            "status": "active",
            "model": null,
            "message_count": 0,
            "token_count_estimate": 0,
            "created_at": "2026-04-10T00:00:00Z",
            "updated_at": "2026-04-10T00:00:00Z"
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path_regex(r"^/api/v1/sessions/[^/]+/messages$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(delay)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse_response(answer_text)),
        )
        .mount(&server)
        .await;

    Mock::given(method("DELETE"))
        .and(path_regex(r"^/api/v1/sessions/[^/]+$"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    // POST /api/v1/knowledge/ingest → 200
    Mock::given(method("POST"))
        .and(path("/api/v1/knowledge/ingest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "inserted": 1,
            "skipped": 0,
            "errors": []
        })))
        .mount(&server)
        .await;

    server
}

async fn setup_ingestion_failure_server() -> MockServer {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/sessions"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": "sess_benchmark_ingest_error",
            "nous_id": "benchmark",
            "session_key": "bench-key",
            "status": "active",
            "model": null,
            "message_count": 0,
            "token_count_estimate": 0,
            "created_at": "2026-04-10T00:00:00Z",
            "updated_at": "2026-04-10T00:00:00Z"
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v1/knowledge/ingest"))
        .respond_with(ResponseTemplate::new(500).set_body_string("ingestion unavailable"))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path_regex(r"^/api/v1/sessions/[^/]+/messages$"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse_response("blue")),
        )
        .mount(&server)
        .await;

    server
}

const SAMPLE_DATASET: &str = r#"[
    {
        "question_id": "q1",
        "question_type": "single-session-user",
        "question": "What color did the user mention?",
        "answer": "blue",
        "haystack_sessions": [
            [
                {"role": "user", "content": "My favorite color is blue"},
                {"role": "assistant", "content": "Noted"}
            ]
        ]
    }
]"#;

const TWO_QUESTION_DATASET: &str = r#"[
    {
        "question_id": "q1",
        "question_type": "single-session-user",
        "question": "What color did alice mention first?",
        "answer": "blue",
        "haystack_sessions": [
            [
                {"role": "user", "content": "Alice mentioned blue first."}
            ]
        ]
    },
    {
        "question_id": "q2",
        "question_type": "single-session-user",
        "question": "What color did bob mention second?",
        "answer": "red",
        "haystack_sessions": [
            [
                {"role": "user", "content": "Bob mentioned red second."}
            ]
        ]
    }
]"#;

#[tokio::test]
async fn runner_scores_perfect_answer() -> TestResult {
    init_crypto();
    let server = setup_mock_server("blue").await;
    let client = EvalClient::new(server.uri(), None);
    let config = BenchmarkRunnerConfig::default();
    let runner = BenchmarkRunner::new(client, config);

    let dataset = LongMemEvalDataset::from_bytes(SAMPLE_DATASET.as_bytes())?;
    let report = runner.run(&dataset).await?;

    assert_eq!(report.total, 1);
    assert!(report.exact_match_rate() > 0.99, "perfect answer should EM");
    assert!(report.mean_f1() > 0.99, "perfect answer should F1=1");
    Ok(())
}

#[tokio::test]
async fn runner_scores_wrong_answer_as_zero() -> TestResult {
    init_crypto();
    let server = setup_mock_server("purple").await;
    let client = EvalClient::new(server.uri(), None);
    let config = BenchmarkRunnerConfig::default();
    let runner = BenchmarkRunner::new(client, config);

    let dataset = LongMemEvalDataset::from_bytes(SAMPLE_DATASET.as_bytes())?;
    let report = runner.run(&dataset).await?;

    assert_eq!(report.total, 1);
    assert!(
        report.exact_match_rate() < 0.01,
        "wrong answer should not EM"
    );
    assert!(report.mean_f1() < 0.01, "wrong answer should have zero F1");
    Ok(())
}

#[tokio::test]
async fn runner_scores_timed_out_question_as_zero() -> TestResult {
    init_crypto();
    let server = setup_slow_mock_server("blue", std::time::Duration::from_millis(200)).await;
    let client = EvalClient::new(server.uri(), None);
    let config = BenchmarkRunnerConfig {
        question_timeout: std::time::Duration::from_millis(100),
        ..Default::default()
    };
    let runner = BenchmarkRunner::new(client, config);

    let dataset = LongMemEvalDataset::from_bytes(SAMPLE_DATASET.as_bytes())?;
    let report = runner.run(&dataset).await?;

    assert_eq!(report.total, 1);
    assert_eq!(report.questions[0].actual_answer, "");
    assert!(
        report.mean_f1().abs() < f64::EPSILON,
        "timeout should be scored as a zero-answer"
    );
    Ok(())
}

#[tokio::test]
async fn runner_marks_ingestion_failure_as_non_scored() -> TestResult {
    init_crypto();
    let server = setup_ingestion_failure_server().await;
    let client = EvalClient::new(server.uri(), None);
    let runner = BenchmarkRunner::new(client, BenchmarkRunnerConfig::default());

    let dataset = LongMemEvalDataset::from_bytes(SAMPLE_DATASET.as_bytes())?;
    let report = runner.run(&dataset).await?;

    assert_eq!(report.total, 1);
    assert_eq!(report.scored, 0);
    assert_eq!(report.ingestion_errors, 1);
    assert_eq!(report.ingestion_partials, 0);
    assert_eq!(report.ingestion_summary.attempts, 1);
    assert_eq!(report.ingestion_summary.failures, 1);

    let question = &report.questions[0];
    assert_eq!(question.status, QuestionStatus::IngestionError);
    assert_eq!(question.actual_answer, "");
    assert!(
        question
            .error_message
            .as_deref()
            .is_some_and(|message| message.contains("transcript ingestion request failed"))
    );
    Ok(())
}

#[tokio::test]
async fn runner_respects_max_questions_limit() -> TestResult {
    init_crypto();
    let server = setup_mock_server("blue").await;
    let client = EvalClient::new(server.uri(), None);
    let config = BenchmarkRunnerConfig {
        max_questions: Some(0),
        ..Default::default()
    };
    let runner = BenchmarkRunner::new(client, config);

    let dataset = LongMemEvalDataset::from_bytes(SAMPLE_DATASET.as_bytes())?;
    let report = runner.run(&dataset).await?;

    assert_eq!(report.total, 0, "max_questions=0 should score nothing");
    Ok(())
}

#[tokio::test]
async fn runner_preserves_question_metadata_in_results() -> TestResult {
    init_crypto();
    let server = setup_mock_server("blue").await;
    let client = EvalClient::new(server.uri(), None);
    let runner = BenchmarkRunner::new(client, BenchmarkRunnerConfig::default());

    let dataset = LongMemEvalDataset::from_bytes(SAMPLE_DATASET.as_bytes())?;
    let report = runner.run(&dataset).await?;

    let question = &report.questions[0];
    assert_eq!(question.id, "q1");
    assert_eq!(question.category, "single-session-user");
    assert_eq!(question.expected_answers, vec!["blue".to_owned()]);
    Ok(())
}

#[tokio::test]
async fn runner_produces_per_category_breakdown() -> TestResult {
    init_crypto();
    let server = setup_mock_server("blue").await;
    let client = EvalClient::new(server.uri(), None);
    let runner = BenchmarkRunner::new(client, BenchmarkRunnerConfig::default());

    let dataset = LongMemEvalDataset::from_bytes(SAMPLE_DATASET.as_bytes())?;
    let report = runner.run(&dataset).await?;

    let per_cat = report.per_category();
    assert_eq!(per_cat.len(), 1);
    assert_eq!(per_cat[0].0, "single-session-user");
    assert!(per_cat[0].1 > 0.99, "single-session-user EM should be 1.0");
    Ok(())
}

#[tokio::test]
async fn runner_rejects_empty_dataset_before_execution() -> TestResult {
    let result = LongMemEvalDataset::from_bytes(b"[]");
    assert!(result.is_err());
    assert!(
        result
            .err()
            .is_some_and(|err| err.to_string().contains("at least one question"))
    );
    Ok(())
}

#[tokio::test]
async fn runner_scores_retrieval_against_evidence_ids() -> TestResult {
    init_crypto();
    let server = setup_mock_server("blue").await;
    Mock::given(method("GET"))
        .and(path("/api/v1/knowledge/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [
                {
                    "id": "fact-blue",
                    "content": "The user stores their favorite pigment in memory.",
                    "confidence": 0.92,
                    "tier": "active",
                    "fact_type": "user",
                    "score": 0.81
                }
            ]
        })))
        .mount(&server)
        .await;
    let client = EvalClient::new(server.uri(), None);
    let config = BenchmarkRunnerConfig {
        retrieval_k: Some(1),
        ..Default::default()
    };
    let runner = BenchmarkRunner::new(client, config);

    let dataset = LongMemEvalDataset::from_bytes(
        r#"[
            {
                "question_id": "q1",
                "question_type": "single-session-user",
                "question": "What color did the user mention?",
                "answer": "blue",
                "evidence_ids": ["fact-blue"],
                "haystack_sessions": [
                    [
                        {"role": "user", "content": "My favorite color is blue"}
                    ]
                ]
            }
        ]"#
        .as_bytes(),
    )?;
    let report = runner.run(&dataset).await?;

    let question = &report.questions[0];
    let recall = question
        .recall_at_k
        .ok_or_else(|| std::io::Error::other("missing recall"))?;
    assert!((recall - 1.0).abs() < f64::EPSILON);
    let scoring = question
        .retrieval_scoring
        .as_ref()
        .ok_or_else(|| std::io::Error::other("missing retrieval scoring"))?;
    assert_eq!(scoring.mode, RetrievalScoringMode::EvidenceId);
    assert!(!scoring.fallback_used);
    let retrieved = &question
        .retrieved_facts
        .as_ref()
        .ok_or_else(|| std::io::Error::other("missing retrieved facts"))?[0];
    assert_eq!(retrieved.id.as_deref(), Some("fact-blue"));
    assert!((retrieved.score - 0.81).abs() < f64::EPSILON);
    assert!(!retrieved.content_sha256.is_empty());
    Ok(())
}

#[tokio::test]
async fn runner_counts_retrieval_search_errors_as_zero_metrics() -> TestResult {
    init_crypto();
    let server = setup_mock_server("blue").await;
    Mock::given(method("GET"))
        .and(path("/api/v1/knowledge/search"))
        .and(query_param("q", "What color did alice mention first?"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [
                {
                    "id": "fact-blue",
                    "content": "blue",
                    "confidence": 0.92,
                    "tier": "active",
                    "fact_type": "user",
                    "score": 0.81
                }
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/knowledge/search"))
        .and(query_param("q", "What color did bob mention second?"))
        .respond_with(ResponseTemplate::new(500).set_body_string("search unavailable"))
        .mount(&server)
        .await;

    let client = EvalClient::new(server.uri(), None);
    let config = BenchmarkRunnerConfig {
        retrieval_k: Some(1),
        ..Default::default()
    };
    let runner = BenchmarkRunner::new(client, config);

    let dataset = LongMemEvalDataset::from_bytes(TWO_QUESTION_DATASET.as_bytes())?;
    let report = runner.run(&dataset).await?;

    assert_eq!(report.total, 2);
    assert_eq!(report.questions[0].recall_at_k, Some(1.0));
    assert_eq!(report.questions[1].recall_at_k, Some(0.0));
    assert_eq!(report.questions[1].ndcg_at_k, Some(0.0));
    let mean_recall = report
        .mean_recall_at_k()
        .ok_or_else(|| std::io::Error::other("missing mean recall"))?;
    let mean_ndcg = report
        .mean_ndcg_at_k()
        .ok_or_else(|| std::io::Error::other("missing mean ndcg"))?;
    assert!((mean_recall - 0.5).abs() < f64::EPSILON);
    assert!((mean_ndcg - 0.5).abs() < f64::EPSILON);
    Ok(())
}
