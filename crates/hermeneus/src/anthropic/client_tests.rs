#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: HashMap string-key indexing; key presence is the assertion under test"
)]
use std::time::Duration;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use aletheia_koina::secret::SecretString;

use super::*;
use crate::anthropic::pricing::{backoff_delay, estimate_cost, model_family};
use crate::error::Error;
use crate::models::BACKOFF_MAX_MS;
use crate::provider::{LlmProvider, ProviderConfig};
use crate::types::{CompletionRequest, Content, Message, Role};

fn test_config_with(base_url: &str) -> ProviderConfig {
    ProviderConfig {
        provider_type: "anthropic".to_owned(),
        api_key: Some(SecretString::from("test-key")),
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
        api_key: Some(SecretString::from("")),
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
        // NOTE: test-only fixture value, not a real credential
        api_key: Some(SecretString::from("sk-test-123")),
        base_url: Some("https://custom.api.example.com".to_owned()),
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let debug = format!("{provider:?}");
    // codequality:ignore — debug output of provider struct contains base_url, not credential values
    assert!(
        debug.contains("custom.api.example.com"),
        "debug should show base_url: {debug}"
    );
}

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
    assert_eq!(
        response.id, "msg_test",
        "response id should match wire response"
    );
    assert_eq!(
        response.stop_reason,
        crate::types::StopReason::EndTurn,
        "stop reason should be EndTurn"
    );
    assert_eq!(
        response.usage.input_tokens, 10,
        "input tokens should match wire response"
    );
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

#[test]
fn estimate_cost_no_pricing_returns_zero() {
    let pricing = HashMap::new();
    assert!(
        estimate_cost(&pricing, "claude-opus-4-20250514", 1000, 100).abs() < f64::EPSILON,
        "opus cost should be zero with no pricing"
    );
    assert!(
        estimate_cost(&pricing, "claude-sonnet-4-20250514", 1000, 100).abs() < f64::EPSILON,
        "sonnet cost should be zero with no pricing"
    );
    assert!(
        estimate_cost(&pricing, "claude-haiku-4-5-20251001", 1000, 100).abs() < f64::EPSILON,
        "haiku cost should be zero with no pricing"
    );
    assert!(
        estimate_cost(&pricing, "some-unknown-model", 1000, 100).abs() < f64::EPSILON,
        "unknown model cost should be zero with no pricing"
    );
}

#[test]
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
    assert!(
        (cost - 0.015).abs() < 0.0001,
        "cost should match expected value for custom pricing"
    );
}

#[test]
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
    assert!(
        (cost - 0.03).abs() < 0.0001,
        "cost should reflect overridden pricing"
    );
}

/// Family resolution: pricing keyed under a versioned alias should apply to
/// any model in the same family.
///
/// Scenario: operator configured pricing for `claude-sonnet-4-6` (the short
/// alias shipped in older configs).  The model actually used is the dated
/// snapshot `claude-sonnet-4-20250514`.  `estimate_cost` must use the
/// `claude-sonnet-4-6` entry rather than returning 0.0.
#[test]
fn estimate_cost_family_resolution_uses_alias_pricing() {
    let mut pricing = HashMap::new();
    pricing.insert(
        "claude-sonnet-4-6".to_owned(),
        ModelPricing {
            input_cost_per_mtok: 3.0,
            output_cost_per_mtok: 15.0,
        },
    );
    let cost = estimate_cost(&pricing, "claude-sonnet-4-20250514", 1_000_000, 0);
    assert!(
        (cost - 3.0).abs() < 0.0001,
        "expected ~$3.00 via family resolution, got {cost}"
    );
}

#[test]
fn estimate_cost_haiku_family_resolution() {
    let mut pricing = HashMap::new();
    pricing.insert(
        "claude-haiku-4-5".to_owned(),
        ModelPricing {
            input_cost_per_mtok: 0.8,
            output_cost_per_mtok: 4.0,
        },
    );
    let cost = estimate_cost(&pricing, "claude-haiku-4-5-20251001", 0, 1_000_000);
    assert!(
        (cost - 4.0).abs() < 0.0001,
        "expected ~$4.00 via family resolution, got {cost}"
    );
}

/// Default pricing table must resolve `claude-haiku-4-5-20251001` via exact
/// match. This is the production scenario: extraction uses Haiku, and the
/// built-in defaults must cover it without relying on family fallback.
#[test]
fn estimate_cost_default_pricing_resolves_haiku() {
    let pricing = ProviderConfig::default().pricing;

    let cost = estimate_cost(&pricing, "claude-haiku-4-5-20251001", 1_000_000, 1_000_000);
    assert!(
        (cost - 4.8).abs() < 0.0001,
        "expected ~$4.80 for haiku from default pricing, got {cost}"
    );

    for model in SUPPORTED_MODELS {
        let c = estimate_cost(&pricing, model, 1000, 1000);
        assert!(
            c > 0.0,
            "default pricing must cover supported model {model}, got cost={c}"
        );
    }
}

#[test]
fn model_family_strips_last_segment() {
    assert_eq!(
        model_family("claude-sonnet-4-20250514"),
        "claude-sonnet-4",
        "dated snapshot should strip to family"
    );
    assert_eq!(
        model_family("claude-sonnet-4-6"),
        "claude-sonnet-4",
        "short alias should strip to family"
    );
    assert_eq!(
        model_family("claude-haiku-4-5-20251001"),
        "claude-haiku-4-5",
        "haiku dated snapshot should strip to family"
    );
    assert_eq!(
        model_family("claude-haiku-4-5"),
        "claude-haiku-4",
        "haiku short should strip to family"
    );
    assert_eq!(
        model_family("claude-opus-4-20250514"),
        "claude-opus-4",
        "opus dated snapshot should strip to family"
    );
    assert_eq!(
        model_family("claude-opus-4-6"),
        "claude-opus-4",
        "opus short should strip to family"
    );
    assert_eq!(
        model_family("somemodel"),
        "somemodel",
        "no-dash model returns unchanged"
    );
}

/// Operator configs often only include pricing for the primary model.
/// Built-in defaults must fill in the rest so background-task models
/// (e.g. Haiku for extraction) always have pricing.
#[test]
fn merge_pricing_fills_defaults_for_unconfigured_models() {
    let config = ProviderConfig {
        api_key: Some(SecretString::from("sk-test")),
        pricing: HashMap::from([(
            "claude-sonnet-4-6".to_owned(),
            ModelPricing {
                input_cost_per_mtok: 99.0,
                output_cost_per_mtok: 99.0,
            },
        )]),
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");

    let sonnet = &provider.pricing["claude-sonnet-4-6"];
    assert!(
        (sonnet.input_cost_per_mtok - 99.0).abs() < f64::EPSILON,
        "operator override should win"
    );

    let haiku = provider
        .pricing
        .get("claude-haiku-4-5-20251001")
        .expect("haiku pricing must be present from defaults");
    assert!(
        (haiku.input_cost_per_mtok - 0.8).abs() < f64::EPSILON,
        "haiku input price should be $0.80/MTok"
    );
    assert!(
        (haiku.output_cost_per_mtok - 4.0).abs() < f64::EPSILON,
        "haiku output price should be $4.00/MTok"
    );
}

/// Empty operator pricing should fall back entirely to built-in defaults.
#[test]
fn merge_pricing_empty_operator_uses_all_defaults() {
    let config = ProviderConfig {
        api_key: Some(SecretString::from("sk-test")),
        pricing: HashMap::new(),
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");

    let defaults = ProviderConfig::default();
    for model in defaults.pricing.keys() {
        assert!(
            provider.pricing.contains_key(model),
            "missing default pricing for {model}"
        );
    }
}

#[test]
fn backoff_delay_respects_retry_after() {
    let err = error::RateLimitedSnafu {
        retry_after_ms: 5000_u64,
    }
    .build();
    let delay = backoff_delay(1, Some(&err));
    assert_eq!(
        delay,
        Duration::from_millis(5000),
        "should use retry-after from rate limit error"
    );
}

#[test]
fn backoff_delay_exponential_growth() {
    let d1 = backoff_delay(1, None);
    let d2 = backoff_delay(2, None);
    let d3 = backoff_delay(3, None);
    assert!(d1 < d2, "attempt 2 delay should exceed attempt 1");
    assert!(d2 < d3, "attempt 3 delay should exceed attempt 2");
    assert!(
        d3 <= Duration::from_millis(BACKOFF_MAX_MS + BACKOFF_MAX_MS / 4),
        "delay should be capped near BACKOFF_MAX_MS"
    );
}
