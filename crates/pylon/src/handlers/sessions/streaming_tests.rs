#![expect(clippy::expect_used, reason = "test assertions")]

use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::*;

/// Default max key length for tests (matches `ApiLimitsConfig::default()`).
const TEST_MAX_KEY_LEN: usize = 64;

fn claims(role: Role, nous_id: Option<&str>) -> Claims {
    Claims {
        sub: "alice".to_owned(),
        role,
        nous_id: nous_id.map(str::to_owned),
    }
}

fn reconnect_path() -> axum::extract::Path<(String, String)> {
    axum::extract::Path(("ses-a".to_owned(), "turn-a".to_owned()))
}

async fn reconnect_test_state() -> (SessionsState, tempfile::TempDir) {
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
    handle.mark_completed().await;

    (state, tmp)
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
        },
        request_id: Some("req-456".to_owned()),
    };
    let result = sse_event_to_axum_with_id((3, event)).expect("infallible");
    drop(result);
}

#[tokio::test]
async fn reconnect_turn_rejects_cross_nous_scoped_caller() {
    let (state, _tmp) = reconnect_test_state().await;

    let blocked = reconnect_turn(
        axum::extract::State(state.clone()),
        claims(Role::Agent, Some("nous-b")),
        HeaderMap::new(),
        reconnect_path(),
    )
    .await;
    let Err(err) = blocked else {
        panic!("cross-nous agent reconnect must be rejected");
    };
    assert_eq!(err.into_response().status(), StatusCode::FORBIDDEN);

    let blocked = reconnect_turn(
        axum::extract::State(state.clone()),
        claims(Role::Operator, Some("nous-b")),
        HeaderMap::new(),
        reconnect_path(),
    )
    .await;
    let Err(err) = blocked else {
        panic!("cross-nous reconnect must be rejected");
    };
    assert_eq!(err.into_response().status(), StatusCode::FORBIDDEN);

    let allowed = reconnect_turn(
        axum::extract::State(state),
        claims(Role::Operator, None),
        HeaderMap::new(),
        reconnect_path(),
    )
    .await;
    assert!(allowed.is_ok(), "operator reconnect should succeed");
}

// ── IdempotencyGuard ──

#[test]
fn idempotency_guard_releases_in_flight_on_drop() {
    let cache = Arc::new(crate::idempotency::IdempotencyCache::new());
    let principal = "alice".to_owned();
    let key = "drop-key".to_owned();

    assert!(
        matches!(cache.check_or_insert(&principal, &key), LookupResult::Miss),
        "precondition: key must be inserted"
    );

    {
        let guard = IdempotencyGuard::new(Arc::clone(&cache), principal.clone(), key.clone());
        assert!(
            matches!(
                cache.check_or_insert(&principal, &key),
                LookupResult::Conflict
            ),
            "key must still be in flight while the guard lives"
        );
        drop(guard);
    }

    assert!(
        matches!(cache.check_or_insert(&principal, &key), LookupResult::Miss),
        "dropping the guard must release the in-flight key"
    );
}

#[test]
fn idempotency_guard_preserves_completed_entry() {
    let cache = Arc::new(crate::idempotency::IdempotencyCache::new());
    let principal = "alice".to_owned();
    let key = "complete-key".to_owned();

    assert!(matches!(
        cache.check_or_insert(&principal, &key),
        LookupResult::Miss
    ));
    cache.complete(
        &principal,
        &key,
        axum::http::StatusCode::OK,
        r#"{"ok":true}"#.to_owned(),
    );

    {
        let guard = IdempotencyGuard::new(Arc::clone(&cache), principal.clone(), key.clone());
        guard.mark_completed();
    }

    assert!(
        matches!(
            cache.check_or_insert(&principal, &key),
            LookupResult::Hit { .. }
        ),
        "mark_completed must prevent the guard from deleting a finished entry"
    );
}

#[test]
fn idempotency_guard_shared_completion_flag() {
    // WHY(#5453): The stream-side and task-side guards share one completion
    // flag so marking the turn completed in the task prevents the stream-side
    // guard from cleaning up after a normal response drop.
    let cache = Arc::new(crate::idempotency::IdempotencyCache::new());
    let principal = "alice".to_owned();
    let key = "shared-key".to_owned();

    assert!(matches!(
        cache.check_or_insert(&principal, &key),
        LookupResult::Miss
    ));

    let (task_guard, stream_guard) =
        IdempotencyGuard::new_pair(Arc::clone(&cache), principal.clone(), key.clone());

    task_guard.mark_completed();
    drop(stream_guard);

    assert!(
        matches!(
            cache.check_or_insert(&principal, &key),
            LookupResult::Conflict
        ),
        "shared completion flag must keep the in-flight entry intact"
    );
}
