#![expect(clippy::expect_used, reason = "test assertions")]
use std::time::Duration;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::error::Error;
use crate::provider::{LlmProvider, ProviderConfig};
use crate::types::{CompletionRequest, Content, Message, Role};

fn test_config_with(base_url: &str) -> ProviderConfig {
    ProviderConfig {
        provider_type: "anthropic".to_owned(),
        api_key: Some("test-key".to_owned()),
        base_url: Some(base_url.to_owned()),
        default_model: None,
        max_retries: Some(0),
        pricing: HashMap::new(),
    }
}

fn test_request() -> CompletionRequest {
    CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        system: None,
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hello".to_owned()),
        }],
        max_tokens: 128,
        tools: vec![],
        temperature: None,
        thinking: None,
        stop_sequences: vec![],
        ..Default::default()
    }
}

fn valid_wire_response_json() -> serde_json::Value {
    serde_json::json!({
        "id": "msg_test",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hello from test"}],
        "model": "claude-opus-4-20250514",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 10, "output_tokens": 5}
    })
}

// --- from_config tests ---

#[test]
fn from_config_missing_api_key() {
    let config = ProviderConfig {
        api_key: None,
        ..ProviderConfig::default()
    };
    let err = AnthropicProvider::from_config(&config).expect_err("should fail without key");
    assert!(
        matches!(err, Error::ProviderInit { .. }),
        "expected ProviderInit, got: {err:?}"
    );
}

#[test]
fn from_config_empty_api_key() {
    let config = ProviderConfig {
        api_key: Some(String::new()),
        ..ProviderConfig::default()
    };
    let err = AnthropicProvider::from_config(&config).expect_err("should fail with empty key");
    assert!(
        matches!(err, Error::ProviderInit { .. }),
        "expected ProviderInit, got: {err:?}"
    );
}

#[test]
fn from_config_valid() {
    let config = ProviderConfig {
        api_key: Some("sk-test-123".to_owned()),
        base_url: Some("https://custom.api.example.com".to_owned()),
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let debug = format!("{provider:?}");
    assert!(
        debug.contains("custom.api.example.com"),
        "debug should show base_url: {debug}"
    );
}

// --- wiremock integration tests ---

#[tokio::test]
async fn complete_success() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(valid_wire_response_json()))
        .expect(1)
        .mount(&server)
        .await;

    let config = test_config_with(&server.uri());
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let response = provider.complete(&test_request()).await.expect("complete");
    assert_eq!(response.id, "msg_test");
    assert_eq!(response.stop_reason, crate::types::StopReason::EndTurn);
    assert_eq!(response.usage.input_tokens, 10);
}

#[tokio::test]
async fn complete_auth_failure_not_retried() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "type": "error",
            "error": {"type": "authentication_error", "message": "invalid api key"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut config = test_config_with(&server.uri());
    config.max_retries = Some(2);
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let err = provider
        .complete(&test_request())
        .await
        .expect_err("should fail");
    assert!(
        matches!(err, Error::AuthFailed { .. }),
        "expected AuthFailed, got: {err:?}"
    );
}

#[tokio::test]
async fn complete_bad_request_not_retried() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "type": "error",
            "error": {"type": "invalid_request_error", "message": "bad input"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut config = test_config_with(&server.uri());
    config.max_retries = Some(2);
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let err = provider
        .complete(&test_request())
        .await
        .expect_err("should fail");
    assert!(
        matches!(err, Error::ApiError { status: 400, .. }),
        "expected ApiError 400, got: {err:?}"
    );
}

#[tokio::test]
async fn complete_rate_limited_no_retry() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(429).set_body_json(serde_json::json!({
            "type": "error",
            "error": {"type": "rate_limit_error", "message": "rate limited"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let config = test_config_with(&server.uri());
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let err = provider
        .complete(&test_request())
        .await
        .expect_err("should fail");
    assert!(
        matches!(err, Error::RateLimited { .. }),
        "expected RateLimited, got: {err:?}"
    );
}

#[tokio::test]
async fn complete_server_error_no_retry() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal server error"))
        .expect(1)
        .mount(&server)
        .await;

    let config = test_config_with(&server.uri());
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let err = provider
        .complete(&test_request())
        .await
        .expect_err("should fail");
    assert!(
        matches!(err, Error::ApiError { status: 500, .. }),
        "expected ApiError 500, got: {err:?}"
    );
}

#[tokio::test]
async fn complete_malformed_body() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json at all"))
        .expect(1)
        .mount(&server)
        .await;

    let config = test_config_with(&server.uri());
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let err = provider
        .complete(&test_request())
        .await
        .expect_err("should fail");
    assert!(
        matches!(err, Error::ParseResponse { .. }),
        "expected ParseResponse, got: {err:?}"
    );
}

// --- estimate_cost unit tests ---

#[test]
#[allow(clippy::float_cmp, reason = "exact zero comparison in pricing test")]
fn estimate_cost_no_pricing_returns_zero() {
    // Without configured pricing, cost is always 0.0 regardless of model.
    let pricing = HashMap::new();
    assert_eq!(
        estimate_cost(&pricing, "claude-opus-4-20250514", 1000, 100),
        0.0
    );
    assert_eq!(
        estimate_cost(&pricing, "claude-sonnet-4-20250514", 1000, 100),
        0.0
    );
    assert_eq!(
        estimate_cost(&pricing, "claude-haiku-4-5-20251001", 1000, 100),
        0.0
    );
    assert_eq!(
        estimate_cost(&pricing, "some-unknown-model", 1000, 100),
        0.0
    );
}

#[test]
#[allow(clippy::float_cmp, reason = "exact comparison in pricing test")]
fn estimate_cost_uses_config_pricing() {
    let mut pricing = HashMap::new();
    pricing.insert(
        "custom-model".to_owned(),
        ModelPricing {
            input_cost_per_mtok: 10.0,
            output_cost_per_mtok: 50.0,
        },
    );
    let cost = estimate_cost(&pricing, "custom-model", 1000, 100);
    // (1000 * 10.0 + 100 * 50.0) / 1_000_000 = 15000 / 1_000_000 = 0.015
    assert!((cost - 0.015).abs() < 0.0001);
}

#[test]
#[allow(clippy::float_cmp, reason = "exact comparison in pricing test")]
fn estimate_cost_config_overrides_default() {
    let mut pricing = HashMap::new();
    pricing.insert(
        "claude-opus-4-20250514".to_owned(),
        ModelPricing {
            input_cost_per_mtok: 20.0,
            output_cost_per_mtok: 100.0,
        },
    );
    let cost = estimate_cost(&pricing, "claude-opus-4-20250514", 1000, 100);
    // (1000 * 20.0 + 100 * 100.0) / 1_000_000 = 30000 / 1_000_000 = 0.03
    assert!((cost - 0.03).abs() < 0.0001);
}

// --- backoff_delay unit tests ---

#[test]
fn backoff_delay_respects_retry_after() {
    let err = error::RateLimitedSnafu {
        retry_after_ms: 5000_u64,
    }
    .build();
    let delay = backoff_delay(1, Some(&err));
    assert_eq!(delay, Duration::from_millis(5000));
}

#[test]
fn backoff_delay_exponential_growth() {
    let d1 = backoff_delay(1, None);
    let d2 = backoff_delay(2, None);
    let d3 = backoff_delay(3, None);
    assert!(d1 < d2, "attempt 2 should be longer than attempt 1");
    assert!(d2 < d3, "attempt 3 should be longer than attempt 2");
    assert!(
        d3 <= Duration::from_millis(BACKOFF_MAX_MS + BACKOFF_MAX_MS / 4),
        "delay should be capped near BACKOFF_MAX_MS"
    );
}
