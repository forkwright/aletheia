#![expect(clippy::expect_used, reason = "test assertions")]

use std::sync::Arc;
use std::time::Duration;

use axum::http::HeaderMap;
use axum::response::IntoResponse;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::*;

/// Default max key length for tests (matches `ApiLimitsConfig::default()`).
const TEST_MAX_KEY_LEN: usize = 64;

pub(super) fn claims(role: Role, nous_id: Option<&str>) -> Claims {
    Claims {
        sub: "alice".to_owned(),
        role,
        nous_id: nous_id.map(str::to_owned),
    }
}

pub(super) fn reconnect_path() -> axum::extract::Path<(String, String)> {
    axum::extract::Path(("ses-a".to_owned(), "turn-a".to_owned()))
}

pub(super) async fn reconnect_running_test_state()
-> (SessionsState, tempfile::TempDir, TurnBufferHandle) {
    let tmp = tempfile::TempDir::new().expect("tmpdir");
    let session_store = Arc::new(Mutex::new(
        mneme::store::SessionStore::open_in_memory().expect("in-memory store"),
    ));
    let provider_registry = Arc::new(hermeneus::provider::ProviderRegistry::new());
    let tool_registry = Arc::new(organon::registry::ToolRegistry::new());
    let oikos = Arc::new(taxis::oikos::Oikos::from_root(tmp.path()));
    let nous_manager = nous::manager::NousManager::new(
        Arc::clone(&provider_registry),
        tool_registry,
        oikos,
        None,
        None,
        Some(Arc::clone(&session_store)),
        #[cfg(feature = "knowledge-store")]
        None,
        Arc::new(vec![]),
        None,
        None,
        taxis::config::NousBehaviorConfig::default(),
        taxis::config::ToolLimitsConfig::default(),
    );
    let config = taxis::config::AletheiaConfig::default();

    let state = SessionsState {
        session_store,
        nous_manager: Arc::new(nous_manager),
        provider_registry,
        shutdown: CancellationToken::new(),
        idempotency_cache: Arc::new(crate::idempotency::IdempotencyCache::new()),
        config: Arc::new(tokio::sync::RwLock::new(config)),
        turn_buffer_registry: Arc::new(crate::turn_buffer::TurnBufferRegistry::new()),
        event_bus: Arc::new(crate::event_bus::EventBus::new(16)),
        approval_registry: Arc::new(crate::approval_registry::ApprovalRegistry::new()),
    };

    state
        .session_store
        .lock()
        .await
        .create_session("ses-a", "nous-a", "main", None, None)
        .expect("create session");
    let buffer = state
        .turn_buffer_registry
        .get_or_create("ses-a", "turn-a")
        .await;
    let handle = TurnBufferHandle::new(buffer);
    handle
        .record("text_delta", r#"{"type":"text_delta","text":"secret"}"#)
        .await;

    (state, tmp, handle)
}

pub(super) async fn reconnect_test_state() -> (SessionsState, tempfile::TempDir) {
    let (state, tmp, handle) = reconnect_running_test_state().await;
    handle.mark_completed().await;
    (state, tmp)
}

pub(super) async fn collect_sse_response(response: axum::response::Response) -> String {
    let bytes = tokio::time::timeout(
        Duration::from_secs(1),
        axum::body::to_bytes(response.into_body(), usize::MAX),
    )
    .await
    .expect("SSE body timed out")
    .expect("SSE body");
    String::from_utf8(bytes.to_vec()).expect("SSE body utf8")
}

pub(super) fn reconnect_path_for(turn_id: &str) -> axum::extract::Path<(String, String)> {
    axum::extract::Path(("ses-a".to_owned(), turn_id.to_owned()))
}

pub(super) async fn reconnect_running_test_state_for(
    turn_id: &str,
) -> (SessionsState, TurnBufferHandle, tempfile::TempDir) {
    let (state, tmp) = reconnect_test_state().await;
    let buffer = state
        .turn_buffer_registry
        .get_or_create("ses-a", turn_id)
        .await;
    let handle = TurnBufferHandle::new(buffer);
    handle
        .record("text_delta", r#"{"type":"text_delta","text":"first"}"#)
        .await;
    (state, handle, tmp)
}

pub(super) async fn response_body(response: impl IntoResponse) -> String {
    let response = response.into_response();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    String::from_utf8(bytes.to_vec()).expect("utf8 body")
}

pub(super) fn parse_sse_data_events(body: &str) -> Vec<serde_json::Value> {
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str(data.trim()).ok())
        .collect()
}

// ── extract_idempotency_key ──

#[test]
fn idempotency_key_absent_returns_none() {
    let headers = HeaderMap::new();
    let result = extract_idempotency_key(&headers, TEST_MAX_KEY_LEN).expect("should succeed");
    assert!(result.is_none());
}

#[test]
fn idempotency_key_present_returns_value() {
    let mut headers = HeaderMap::new();
    headers.insert("idempotency-key", "abc-123".parse().expect("valid header"));
    let result = extract_idempotency_key(&headers, TEST_MAX_KEY_LEN).expect("should succeed");
    assert_eq!(result.as_deref(), Some("abc-123"));
}

#[test]
fn idempotency_key_empty_value_rejected() {
    let mut headers = HeaderMap::new();
    headers.insert("idempotency-key", "".parse().expect("valid header"));
    let result = extract_idempotency_key(&headers, TEST_MAX_KEY_LEN);
    assert!(result.is_err(), "empty key should be rejected");
}

#[test]
fn idempotency_key_too_long_rejected() {
    let mut headers = HeaderMap::new();
    let long_key = "a".repeat(TEST_MAX_KEY_LEN + 1);
    headers.insert(
        "idempotency-key",
        long_key.parse().expect("valid ascii header"),
    );
    let result = extract_idempotency_key(&headers, TEST_MAX_KEY_LEN);
    assert!(result.is_err(), "over-long key should be rejected");
}

#[test]
fn idempotency_key_at_max_length_accepted() {
    let mut headers = HeaderMap::new();
    let key = "a".repeat(TEST_MAX_KEY_LEN);
    headers.insert("idempotency-key", key.parse().expect("valid header"));
    let result = extract_idempotency_key(&headers, TEST_MAX_KEY_LEN).expect("should succeed");
    assert!(result.is_some());
}

#[test]
fn idempotency_key_case_insensitive_header() {
    // NOTE: HTTP headers are case-insensitive; axum normalizes them.
    let mut headers = HeaderMap::new();
    headers.insert(
        "Idempotency-Key",
        "mixed-case".parse().expect("valid header"),
    );
    let result = extract_idempotency_key(&headers, TEST_MAX_KEY_LEN).expect("should succeed");
    assert_eq!(result.as_deref(), Some("mixed-case"));
}

// ── classify_llm_error ──

#[expect(
    clippy::unnecessary_box_returns,
    reason = "ApiSnafu requires Box<ApiErrorContext> in its context field"
)]
fn make_api_context() -> Box<hermeneus::error::ApiErrorContext> {
    Box::new(hermeneus::error::ApiErrorContext {
        model: "claude-opus".to_owned(),
        credential_source: "environment".to_owned(),
    })
}

#[test]
fn llm_error_rate_limited_classified() {
    use snafu::IntoError;
    let err = hermeneus::error::RateLimitedSnafu {
        retry_after_ms: 60_000_u64,
    }
    .into_error(snafu::NoneError);
    let (code, msg) = classify_llm_error(&err);
    assert_eq!(code, "rate_limited");
    assert!(
        msg.contains("60000"),
        "message should include retry_after_ms"
    );
}

#[test]
fn llm_error_api_429_classified_as_rate_limited() {
    use snafu::IntoError;
    let err = hermeneus::error::ApiSnafu {
        status: 429_u16,
        message: "Too Many Requests".to_owned(),
        context: make_api_context(),
    }
    .into_error(snafu::NoneError);
    let (code, _) = classify_llm_error(&err);
    assert_eq!(code, "rate_limited");
}

#[test]
fn llm_error_auth_failed_classified() {
    use snafu::IntoError;
    let err = hermeneus::error::AuthFailedSnafu {
        message: "bad key".to_owned(),
    }
    .into_error(snafu::NoneError);
    let (code, msg) = classify_llm_error(&err);
    assert_eq!(code, "auth_failure");
    assert!(msg.contains("authentication"));
}

#[test]
fn llm_error_api_503_classified_as_provider_unavailable() {
    use snafu::IntoError;
    let err = hermeneus::error::ApiSnafu {
        status: 503_u16,
        message: "Service Unavailable".to_owned(),
        context: make_api_context(),
    }
    .into_error(snafu::NoneError);
    let (code, msg) = classify_llm_error(&err);
    assert_eq!(code, "provider_unavailable");
    assert!(msg.contains("503"), "message should include status code");
}

#[test]
fn llm_error_api_500_classified_as_provider_error() {
    use snafu::IntoError;
    let err = hermeneus::error::ApiSnafu {
        status: 500_u16,
        message: "Internal Server Error".to_owned(),
        context: make_api_context(),
    }
    .into_error(snafu::NoneError);
    let (code, msg) = classify_llm_error(&err);
    assert_eq!(code, "provider_error");
    assert!(msg.contains("500"), "message should include status code");
    assert!(
        msg.contains("Internal Server Error"),
        "message should include provider detail"
    );
}

#[test]
fn llm_error_api_400_classified_as_invalid_request() {
    use snafu::IntoError;
    let err = hermeneus::error::ApiSnafu {
        status: 400_u16,
        message: "max tokens exceeded".to_owned(),
        context: make_api_context(),
    }
    .into_error(snafu::NoneError);
    let (code, msg) = classify_llm_error(&err);
    assert_eq!(code, "invalid_request");
    assert!(msg.contains("400"));
    assert!(msg.contains("max tokens exceeded"));
}

#[test]
fn llm_error_unsupported_model_classified() {
    use snafu::IntoError;
    let err = hermeneus::error::UnsupportedModelSnafu {
        model: "gpt-99".to_owned(),
    }
    .into_error(snafu::NoneError);
    let (code, msg) = classify_llm_error(&err);
    assert_eq!(code, "unsupported_model");
    assert!(msg.contains("gpt-99"));
}

#[test]
fn llm_error_api_request_timeout_classified() {
    use snafu::IntoError;
    let err = hermeneus::error::ApiRequestSnafu {
        message: "connection timeout after 30s".to_owned(),
    }
    .into_error(snafu::NoneError);
    let (code, msg) = classify_llm_error(&err);
    assert_eq!(code, "provider_timeout");
    assert!(msg.contains("timed out"));
}

#[test]
fn llm_error_api_request_non_timeout_classified() {
    use snafu::IntoError;
    let err = hermeneus::error::ApiRequestSnafu {
        message: "connection refused".to_owned(),
    }
    .into_error(snafu::NoneError);
    let (code, _) = classify_llm_error(&err);
    assert_eq!(code, "provider_error");
}

#[test]
fn llm_error_api_500_redacts_secrets_in_message() {
    use snafu::IntoError;
    let err = hermeneus::error::ApiSnafu {
        status: 500_u16,
        message: "invalid key sk-ant-abc123def456".to_owned(),
        context: make_api_context(),
    }
    .into_error(snafu::NoneError);
    let (_, msg) = classify_llm_error(&err);
    // WHY(#844): secrets must be redacted from client-visible messages
    assert!(
        !msg.contains("sk-ant-abc123def456"),
        "API key must be redacted"
    );
    assert!(msg.contains("[REDACTED]"));
}

// ── turn_error_info: nous error dispatch ──

#[test]
fn nous_pipeline_timeout_classified() {
    use snafu::IntoError;
    let err = nous::error::PipelineTimeoutSnafu {
        stage: "execute".to_owned(),
        timeout_secs: 30_u32,
    }
    .into_error(snafu::NoneError);
    let (code, msg) = turn_error_info(&err);
    assert_eq!(code, "turn_timeout");
    assert!(msg.contains("execute"), "message should include stage name");
    assert!(
        msg.contains("30"),
        "message should include timeout duration"
    );
}

#[test]
fn nous_ask_timeout_classified() {
    use snafu::IntoError;
    let err = nous::error::AskTimeoutSnafu {
        nous_id: "target".to_owned(),
        timeout_secs: 10_u64,
    }
    .into_error(snafu::NoneError);
    let (code, msg) = turn_error_info(&err);
    assert_eq!(code, "turn_timeout");
    assert!(
        msg.contains("target"),
        "message should include target nous_id"
    );
}

#[test]
fn nous_inbox_full_classified_as_service_busy() {
    use snafu::IntoError;
    let err = nous::error::InboxFullSnafu {
        nous_id: "syn".to_owned(),
        timeout_secs: 30_u64,
    }
    .into_error(snafu::NoneError);
    let (code, _) = turn_error_info(&err);
    assert_eq!(code, "service_busy");
}

#[test]
fn nous_context_assembly_classified() {
    let err = nous::error::ContextAssemblySnafu {
        message: "SOUL.md missing",
    }
    .build();
    let (code, msg) = turn_error_info(&err);
    assert_eq!(code, "context_error");
    assert!(msg.contains("SOUL.md missing"));
}

#[test]
fn nous_loop_detected_classified() {
    let err = nous::error::LoopDetectedSnafu {
        iterations: 5_u32,
        pattern: "exec:abc123",
    }
    .build();
    let (code, msg) = turn_error_info(&err);
    assert_eq!(code, "loop_detected");
    assert!(msg.contains("5 iterations"));
    assert!(msg.contains("exec:abc123"));
}

#[test]
fn nous_pipeline_stage_classified() {
    let err = nous::error::PipelineStageSnafu {
        stage: "recall",
        message: "embedding service down",
    }
    .build();
    let (code, msg) = turn_error_info(&err);
    assert_eq!(code, "pipeline_error");
    assert!(msg.contains("recall"));
    assert!(msg.contains("embedding service down"));
}

#[test]
fn nous_guard_rejected_includes_reason() {
    let err = nous::error::GuardRejectedSnafu {
        reason: "token limit exceeded",
    }
    .build();
    let (code, msg) = turn_error_info(&err);
    assert_eq!(code, "guard_rejected");
    assert!(msg.contains("token limit exceeded"));
}

// ── redact_secrets ──

#[test]
fn redact_strips_anthropic_api_key() {
    let msg = "invalid key sk-ant-abc123def456ghi789"; // pii-allow: synthetic Anthropic key shape, redactor self-test
    let redacted = redact_secrets(msg);
    assert!(
        !redacted.contains("sk-ant-"),
        "API key prefix should be redacted"
    );
    assert!(redacted.contains("[REDACTED]"));
}

#[test]
fn redact_strips_generic_sk_key() {
    let msg = "auth error with sk-abcdefghijklmnopqrstuvwxyz"; // pii-allow: synthetic generic sk- shape, redactor self-test
    let redacted = redact_secrets(msg);
    assert!(
        !redacted.contains("sk-abcdef"),
        "sk- key should be redacted"
    );
    assert!(redacted.contains("[REDACTED]"));
}

#[test]
fn redact_preserves_normal_messages() {
    let msg = "connection timeout after 30s";
    let redacted = redact_secrets(msg);
    assert_eq!(redacted, msg);
}

#[test]
fn redact_strips_bearer_token() {
    let msg = "rejected bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload"; // pii-allow: synthetic Bearer/JWT shape, redactor self-test
    let redacted = redact_secrets(msg);
    assert!(
        !redacted.contains("eyJh"),
        "bearer token should be redacted"
    );
}

// ── sse_event_to_axum_with_id: serialization ──

#[test]
fn sse_event_text_delta_serializes_correctly() {
    let event = SseEvent::TextDelta {
        text: "hello world".to_owned(),
    };
    let result = sse_event_to_axum_with_id((1, event)).expect("infallible");
    // WHY: axum::response::sse::Event fields are not inspectable; the test
    // verifies the conversion does not panic.
    drop(result);
}

#[test]
fn sse_event_error_serializes_correctly() {
    let event = SseEvent::Error {
        code: "turn_failed".to_owned(),
        message: "something broke".to_owned(),
        request_id: Some("req-abc".to_owned()),
    };
    let result = sse_event_to_axum_with_id((2, event)).expect("infallible");
    drop(result);
}

#[test]
fn sse_event_message_complete_serializes_correctly() {
    let event = SseEvent::MessageComplete {
        stop_reason: "end_turn".to_owned(),
        usage: UsageData {
            input_tokens: 100,
            output_tokens: 200,
            cache_read_tokens: 50,
            cache_write_tokens: 25,
        },
        request_id: Some("req-456".to_owned()),
    };
    let result = sse_event_to_axum_with_id((3, event)).expect("infallible");
    drop(result);
}

#[test]
fn turn_complete_event_payload_includes_partial_stop_reason() {
    let result = nous::pipeline::TurnResult {
        content: "partial response".to_owned(),
        tool_calls: Vec::new(),
        usage: nous::pipeline::TurnUsage {
            input_tokens: 10,
            output_tokens: 20,
            ..nous::pipeline::TurnUsage::default()
        },
        signals: Vec::new(),
        stop_reason: "max_tool_iterations".to_owned(),
        degraded: None,
        reasoning: String::new(),
        model_used: "test-model".to_owned(),
        tool_surface_hashes: Vec::new(),
    };

    let payload = turn_complete_event_payload("ses-1", "nous-1", "turn-1", &result);

    assert_eq!(
        payload
            .get("session_id")
            .and_then(serde_json::Value::as_str),
        Some("ses-1")
    );
    assert_eq!(
        payload.get("nous_id").and_then(serde_json::Value::as_str),
        Some("nous-1")
    );
    assert_eq!(
        payload.get("turn_id").and_then(serde_json::Value::as_str),
        Some("turn-1")
    );
    assert_eq!(
        payload
            .get("input_tokens")
            .and_then(serde_json::Value::as_u64),
        Some(10)
    );
    assert_eq!(
        payload
            .get("output_tokens")
            .and_then(serde_json::Value::as_u64),
        Some(20)
    );
    assert_eq!(
        payload
            .get("stop_reason")
            .and_then(serde_json::Value::as_str),
        Some("max_tool_iterations")
    );
}

#[test]
fn approval_required_event_payload_omits_tool_input() {
    let payload = approval_required_event_payload(
        "ses-1",
        "nous-1",
        "turn-1",
        "tool-1",
        "write_file",
        "high",
        "Tool 'write_file' requires required approval",
    );

    assert_eq!(
        payload
            .get("session_id")
            .and_then(serde_json::Value::as_str),
        Some("ses-1")
    );
    assert_eq!(
        payload.get("tool_name").and_then(serde_json::Value::as_str),
        Some("write_file")
    );
    assert!(
        payload.get("input").is_none(),
        "global approval-required events must not include raw tool input"
    );
}

#[path = "streaming_reconnect_tests.rs"]
mod reconnect_tests;
