//! End-to-end integration tests for the benchmark runner using a mock server.
//!
//! These tests spin up a wiremock server that impersonates the aletheia API
//! enough for the benchmark runner to exercise the full pipeline: session
//! creation → haystack ingestion → question → SSE response → scoring.

#![allow(clippy::unwrap_used, reason = "integration test boilerplate")]
#![allow(clippy::expect_used, reason = "integration test boilerplate")]
#![allow(clippy::indexing_slicing, reason = "integration test boilerplate")]

use dokimion::benchmarks::longmemeval::LongMemEvalDataset;
use dokimion::benchmarks::{BenchmarkRunner, BenchmarkRunnerConfig, EvalClient, MemoryBenchmark};
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Initialize the rustls default crypto provider once per test run.
/// Required because the workspace uses `reqwest` with `rustls-no-provider`.
fn init_crypto() {
    // install_default() is idempotent: subsequent calls return Err and are ignored.
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Build an SSE response body that emits a single `text_delta` and a
/// `message_complete` event. This mimics the aletheia streaming format.
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
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(serde_json::json!({
                    "id": "sess_benchmark_123",
                    "nous_id": "benchmark",
                    "session_key": "bench-key",
                    "status": "active",
                    "model": null,
                    "message_count": 0,
                    "token_count_estimate": 0,
                    "created_at": "2026-04-10T00:00:00Z",
                    "updated_at": "2026-04-10T00:00:00Z"
                })),
        )
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

    server
}

const SAMPLE_DATASET: &str = r#"[
    {
        "question_id": "q1",
        "question_type": "factual",
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

#[tokio::test]
async fn runner_scores_perfect_answer() {
    init_crypto();
    let server = setup_mock_server("blue").await;
    let client = EvalClient::new(server.uri(), None);
    let config = BenchmarkRunnerConfig::default();
    let runner = BenchmarkRunner::new(client, config);

    let dataset =
        LongMemEvalDataset::from_bytes(SAMPLE_DATASET.as_bytes()).expect("valid dataset");
    let report = runner.run(&dataset).await.expect("run should succeed");

    assert_eq!(report.total, 1);
    assert!(report.exact_match_rate() > 0.99, "perfect answer should EM");
    assert!(report.mean_f1() > 0.99, "perfect answer should F1=1");
}

#[tokio::test]
async fn runner_scores_wrong_answer_as_zero() {
    init_crypto();
    let server = setup_mock_server("purple").await;
    let client = EvalClient::new(server.uri(), None);
    let config = BenchmarkRunnerConfig::default();
    let runner = BenchmarkRunner::new(client, config);

    let dataset =
        LongMemEvalDataset::from_bytes(SAMPLE_DATASET.as_bytes()).expect("valid dataset");
    let report = runner.run(&dataset).await.expect("run should succeed");

    assert_eq!(report.total, 1);
    assert!(report.exact_match_rate() < 0.01, "wrong answer should not EM");
    assert!(report.mean_f1() < 0.01, "wrong answer should have zero F1");
}

#[tokio::test]
async fn runner_respects_max_questions_limit() {
    init_crypto();
    let server = setup_mock_server("blue").await;
    let client = EvalClient::new(server.uri(), None);
    let config = BenchmarkRunnerConfig {
        max_questions: Some(0),
        ..Default::default()
    };
    let runner = BenchmarkRunner::new(client, config);

    let dataset =
        LongMemEvalDataset::from_bytes(SAMPLE_DATASET.as_bytes()).expect("valid dataset");
    let report = runner.run(&dataset).await.expect("run should succeed");

    assert_eq!(report.total, 0, "max_questions=0 should score nothing");
}

#[tokio::test]
async fn runner_preserves_question_metadata_in_results() {
    init_crypto();
    let server = setup_mock_server("blue").await;
    let client = EvalClient::new(server.uri(), None);
    let runner = BenchmarkRunner::new(client, BenchmarkRunnerConfig::default());

    let dataset =
        LongMemEvalDataset::from_bytes(SAMPLE_DATASET.as_bytes()).expect("valid dataset");
    let report = runner.run(&dataset).await.expect("run should succeed");

    let question = &report.questions[0];
    assert_eq!(question.id, "q1");
    assert_eq!(question.category, "factual");
    assert_eq!(question.expected_answers, vec!["blue".to_owned()]);
}

#[tokio::test]
async fn runner_produces_per_category_breakdown() {
    init_crypto();
    let server = setup_mock_server("blue").await;
    let client = EvalClient::new(server.uri(), None);
    let runner = BenchmarkRunner::new(client, BenchmarkRunnerConfig::default());

    let dataset =
        LongMemEvalDataset::from_bytes(SAMPLE_DATASET.as_bytes()).expect("valid dataset");
    let report = runner.run(&dataset).await.expect("run should succeed");

    let per_cat = report.per_category();
    assert_eq!(per_cat.len(), 1);
    assert_eq!(per_cat[0].0, "factual");
    assert!(per_cat[0].1 > 0.99, "factual EM should be 1.0");
}

#[tokio::test]
async fn runner_handles_empty_dataset() {
    init_crypto();
    let server = setup_mock_server("anything").await;
    let client = EvalClient::new(server.uri(), None);
    let runner = BenchmarkRunner::new(client, BenchmarkRunnerConfig::default());

    let dataset = LongMemEvalDataset::from_bytes(b"[]").expect("valid empty dataset");
    assert!(dataset.is_empty());
    let report = runner.run(&dataset).await.expect("run should succeed");

    assert_eq!(report.total, 0);
    assert!((report.exact_match_rate()).abs() < f64::EPSILON);
    assert!((report.mean_f1()).abs() < f64::EPSILON);
}
