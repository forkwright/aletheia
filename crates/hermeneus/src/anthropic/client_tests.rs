#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: HashMap string-key indexing; key presence is the assertion under test"
)]
use std::borrow::Cow;
use std::time::Duration;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use koina::secret::SecretString;

use super::*;
use crate::anthropic::pricing::{estimate_cost, model_family};
use crate::error::Error;
use crate::models::BACKOFF_MAX_MS;
use crate::provider::{DeploymentTarget, LlmProvider, MatchKind, ProviderConfig};
use crate::types::{CompletionRequest, Content, Message, Role};

fn test_config_with(base_url: &str) -> ProviderConfig {
    ProviderConfig {
        provider_type: "anthropic".to_owned(),
        api_key: Some(SecretString::from("test-key")),
        base_url: Some(base_url.to_owned()),
        default_model: None,
        retry_policy: crate::RetryPolicy {
            max_retries: 0,
            ..crate::RetryPolicy::default()
        },
        concurrency: crate::concurrency::ConcurrencyConfig::default(),
        pricing: HashMap::new(),
        cc_mimicry: None,
        prompt_cache_mode: crate::provider::PromptCacheMode::Disabled,
        deployment_target: crate::provider::DeploymentTarget::Cloud,
        name: None,
        models: Vec::new(),
    }
}

fn test_request() -> CompletionRequest {
    CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        system: None,
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hello".to_owned()),
            cache_breakpoint: false,
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
fn from_config_rejects_spoofed_loopback_without_leaking_credentials() {
    let config = ProviderConfig {
        api_key: Some(SecretString::from("sk-secret-5055")),
        base_url: Some("http://operator:url-secret@127.0.0.1.evil.example/v1".to_owned()), // pii-allow: test fixture asserting credential non-leak
        ..ProviderConfig::default()
    };
    let err = AnthropicProvider::from_config(&config).expect_err("spoofed host must fail");
    let message = err.to_string();

    assert!(message.contains("HTTPS"), "unexpected error: {message}");
    assert!(
        message.contains("127.0.0.1.evil.example"),
        "diagnostic should keep the rejected host: {message}"
    );
    assert!(
        !message.contains("url-secret"),
        "diagnostic must redact URL userinfo: {message}"
    );
    assert!(
        !message.contains("sk-secret-5055"),
        "diagnostic must not include API keys: {message}"
    );
}

#[test]
fn with_credential_provider_rejects_spoofed_loopback_without_leaking_credentials() {
    use std::sync::Arc;

    let config = ProviderConfig {
        base_url: Some("http://operator:url-secret@localhost.evil.example/v1".to_owned()), // pii-allow: test fixture asserting credential non-leak
        ..ProviderConfig::default()
    };
    let credential_provider = Arc::new(StaticCredentialProvider {
        key: SecretString::from("dynamic-secret-5055"),
    });

    let err = AnthropicProvider::with_credential_provider(credential_provider, &config)
        .expect_err("spoofed host must fail");
    let message = err.to_string();

    assert!(message.contains("HTTPS"), "unexpected error: {message}");
    assert!(
        message.contains("localhost.evil.example"),
        "diagnostic should keep the rejected host: {message}"
    );
    assert!(
        !message.contains("url-secret"),
        "diagnostic must redact URL userinfo: {message}"
    );
    assert!(
        !message.contains("dynamic-secret-5055"),
        "diagnostic must not include credential provider secrets: {message}"
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
    // codequality:ignore -- debug output of provider struct contains base_url, not credential values
    assert!(
        debug.contains("custom.api.example.com"),
        "debug should show base_url: {debug}"
    );
}

#[test]
fn from_config_custom_models_claim_routing() {
    let config = ProviderConfig {
        // NOTE: test-only fixture value, not a real credential
        api_key: Some(SecretString::from("sk-test-123")),
        base_url: Some("https://compat.api.example.com".to_owned()),
        name: Some("kimi-coding".to_owned()),
        models: vec!["kimi-for-coding".to_owned()],
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    assert_eq!(provider.name(), "kimi-coding");
    // WHY (#5259): config-owned model IDs are no longer exposed through
    // `supported_models()` as `&[&str]` (that would require leaking). They are
    // still reachable via `supported_model_list()` and routing uses
    // `match_specificity()`.
    assert_eq!(
        provider
            .supported_model_list()
            .iter()
            .map(std::convert::AsRef::as_ref)
            .collect::<Vec<_>>(),
        ["kimi-for-coding"]
    );
    assert!(provider.supports_model("kimi-for-coding"));
    assert_eq!(
        provider.match_specificity("kimi-for-coding"),
        Some(MatchKind::Exact)
    );
    assert_eq!(
        provider.match_specificity(koina::models::names::opus()),
        Some(MatchKind::CatchAll),
        "custom-model instance catches claude-* at lower precedence"
    );
}

#[test]
fn from_config_default_models_and_name_unchanged() {
    let config = ProviderConfig {
        // NOTE: test-only fixture value, not a real credential
        api_key: Some(SecretString::from("sk-test-123")),
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    assert_eq!(provider.name(), "anthropic");
    assert!(provider.supports_model("claude-opus-4-6"));
    // WHY (#4881): built-in Anthropic catalog models are exact matches,
    // not catch-all, so they win over broad claude-* providers.
    assert_eq!(
        provider.match_specificity("claude-opus-4-6"),
        Some(MatchKind::Exact)
    );
    assert_eq!(
        provider.match_specificity("claude-future-family-model"),
        Some(MatchKind::CatchAll)
    );
    assert_eq!(provider.match_specificity("kimi-for-coding"), None);
}

#[test]
fn from_config_deployment_target_propagates() {
    let config = ProviderConfig {
        // NOTE: test-only fixture value, not a real credential
        api_key: Some(SecretString::from("sk-test-123")),
        base_url: Some("https://compat.api.example.com".to_owned()),
        deployment_target: DeploymentTarget::LocalHosted,
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    assert_eq!(provider.deployment_target(), DeploymentTarget::LocalHosted);
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
#[expect(
    deprecated,
    reason = "set_linger(Duration::ZERO) sends RST immediately and does not block; test-only"
)]
async fn complete_streaming_retries_pre_content_connection_reset() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let addr = listener.local_addr().expect("read listener address");
    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_server = Arc::clone(&call_count);

    let good_sse = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_sse_retry\",\"model\":\"claude-opus-4-20250514\",\"usage\":{\"input_tokens\":3,\"output_tokens\":0}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Ok\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    let server = tokio::spawn(async move {
        {
            let (mut socket, _) = listener.accept().await.expect("accept attempt 1");
            call_count_server.fetch_add(1, Ordering::SeqCst);
            let mut buf = vec![0_u8; 4096];
            loop {
                let n = socket.read(&mut buf).await.expect("read request 1");
                if n == 0 || buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n")
                .await
                .expect("write attempt 1 headers");
            socket
                .set_linger(Some(Duration::ZERO))
                .expect("set reset linger");
            drop(socket);
        }

        {
            let (mut socket, _) = listener.accept().await.expect("accept attempt 2");
            call_count_server.fetch_add(1, Ordering::SeqCst);
            let mut buf = vec![0_u8; 4096];
            loop {
                let n = socket.read(&mut buf).await.expect("read request 2");
                if n == 0 || buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                good_sse.len(),
                good_sse
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write retry response");
        }
    });

    let mut config = test_config_with(&format!("http://{addr}"));
    config.retry_policy.max_retries = 1;
    let provider = AnthropicProvider::from_config(&config).expect("valid config");

    let mut events = Vec::new();
    let response = provider
        .complete_streaming(&test_request(), |event| events.push(event))
        .await
        .expect("streaming should retry after pre-content reset");

    server.await.expect("server task completes");

    assert_eq!(
        call_count.load(Ordering::SeqCst),
        2,
        "pre-content connection reset should be retried once"
    );
    assert_eq!(response.id, "msg_sse_retry");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamEvent::TextDelta { text } if text == "Ok")),
        "retry response text delta should be emitted: {events:?}"
    );
}

/// #3406: every outbound request must carry a training opt-out header.
/// Sovereignty default — not configurable.
#[tokio::test]
async fn request_carries_training_optout_header() {
    use wiremock::matchers::header_exists;

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header_exists("anthropic-disable-training"))
        .and(header_exists("anthropic-training-opt-out"))
        .respond_with(ResponseTemplate::new(200).set_body_json(valid_wire_response_json()))
        .expect(1)
        .mount(&server)
        .await;

    let config = test_config_with(&server.uri());
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    provider
        .complete(&test_request())
        .await
        .expect("complete ok");
}

/// #3410: when `prompt_cache_mode` = `Disabled` (default), the serialized
/// request body must contain no `cache_control` markers regardless of
/// what the caller put on the `CompletionRequest`.
#[tokio::test]
async fn disabled_prompt_cache_strips_cache_control_markers() {
    use wiremock::matchers::body_string_contains;

    let server = MockServer::start().await;

    // Any request that slips a `cache_control` marker through fails to
    // match and causes the mock's .expect(1) check to fire.
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(valid_wire_response_json()))
        .expect(1)
        .mount(&server)
        .await;

    // Negative mock: if the body ever contains cache_control we would
    // route to this 500 responder instead.
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(body_string_contains("cache_control"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let config = test_config_with(&server.uri());
    assert_eq!(
        config.prompt_cache_mode,
        crate::provider::PromptCacheMode::Disabled,
        "test fixture default must be Disabled"
    );
    let provider = AnthropicProvider::from_config(&config).expect("valid config");

    // Caller sets every cache flag; provider must scrub them.
    let mut req = test_request();
    req.cache_system = true;
    req.cache_tools = true;
    req.cache_turns = true;

    provider.complete(&req).await.expect("complete ok");
}

/// #3410: when `prompt_cache_mode` = `Ephemeral`, caller-provided cache
/// flags are honored and `cache_control` markers appear in the wire body.
#[tokio::test]
async fn ephemeral_prompt_cache_preserves_cache_control_markers() {
    use wiremock::matchers::body_string_contains;

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(body_string_contains("cache_control"))
        .respond_with(ResponseTemplate::new(200).set_body_json(valid_wire_response_json()))
        .expect(1)
        .mount(&server)
        .await;

    let mut config = test_config_with(&server.uri());
    config.prompt_cache_mode = crate::provider::PromptCacheMode::Ephemeral;
    let provider = AnthropicProvider::from_config(&config).expect("valid config");

    let mut req = test_request();
    // system: field with cache_system = true triggers the array form with
    // cache_control on the block.
    req.system = Some("operator system prompt".to_owned());
    req.cache_system = true;

    provider.complete(&req).await.expect("complete ok");
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
    config.retry_policy.max_retries = 2;
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
    config.retry_policy.max_retries = 2;
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
            input_cost_per_mtok: 1.0,
            output_cost_per_mtok: 5.0,
        },
    );
    let cost = estimate_cost(&pricing, "claude-haiku-4-5-20251001", 0, 1_000_000);
    assert!(
        (cost - 5.0).abs() < 0.0001,
        "expected ~$5.00 via family resolution, got {cost}"
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
        (cost - 6.0).abs() < 0.0001,
        "expected ~$6.00 for haiku from default pricing, got {cost}"
    );

    for model in koina::models::provider_models(koina::models::ModelProvider::Anthropic) {
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
        (haiku.input_cost_per_mtok - 1.0).abs() < f64::EPSILON,
        "haiku input price should be $1.00/MTok"
    );
    assert!(
        (haiku.output_cost_per_mtok - 5.0).abs() < f64::EPSILON,
        "haiku output price should be $5.00/MTok"
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
    let delay = crate::RetryPolicy::default().delay(1, Some(&err));
    assert_eq!(
        delay,
        Duration::from_secs(5),
        "should use retry-after from rate limit error"
    );
}

#[test]
fn backoff_delay_exponential_growth() {
    let policy = crate::RetryPolicy::default();
    let d1 = policy.delay(1, None);
    let d2 = policy.delay(2, None);
    let d3 = policy.delay(3, None);
    assert!(d1 < d2, "attempt 2 delay should exceed attempt 1");
    assert!(d2 < d3, "attempt 3 delay should exceed attempt 2");
    assert!(
        d3 <= Duration::from_millis(BACKOFF_MAX_MS + BACKOFF_MAX_MS / 4),
        "delay should be capped near BACKOFF_MAX_MS"
    );
}

#[test]
fn custom_models_exposed_without_leaking() {
    // WHY (#5259): config-owned model IDs must not be intentionally leaked.
    // `supported_models()` returns `&[]` for custom lists; the diagnostic
    // `supported_model_list()` returns owned `Cow` values freed on drop.
    let config = ProviderConfig {
        api_key: Some(SecretString::from("test-key")),
        models: vec!["custom-model".to_owned()],
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    assert!(provider.supported_models().is_empty());
    let list = provider.supported_model_list();
    assert!(
        list.iter().any(|m| m.as_ref() == "custom-model"),
        "config model must be enumerable"
    );
}

#[test]
fn default_models_are_borrowed_cows() {
    // WHY (#5259): first-party/static model lists should be borrowed, not
    // leaked copies of static data.
    let config = ProviderConfig {
        api_key: Some(SecretString::from("test-key")),
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    assert!(!provider.supported_models().is_empty());
    let list = provider.supported_model_list();
    assert!(!list.is_empty());
    assert!(
        list.iter().all(|m| matches!(m, Cow::Borrowed(_))),
        "default Anthropic catalog must be borrowed, not owned/leaked"
    );
}

#[test]
fn repeated_construction_does_not_leak_model_storage() {
    // WHY (#5259): long-running harnesses may construct providers many times.
    // Each construction must be able to free its model list.
    for i in 0..100 {
        let config = ProviderConfig {
            api_key: Some(SecretString::from("test-key")),
            models: vec![format!("custom-model-{i}")],
            ..ProviderConfig::default()
        };
        let provider = AnthropicProvider::from_config(&config).expect("valid config");
        assert!(provider.supports_model(&format!("custom-model-{i}")));
        // provider drops here, freeing the owned model list.
    }
}

/// #5886: `compute_attribution` must return `None` for non-OAuth credentials.
///
/// Before the fix, `cc_profile` was set at construction time whenever the
/// endpoint was first-party + `cc_mimicry` enabled, and `compute_attribution`
/// gated only on `cc_profile` presence — not on the runtime credential source.
/// This caused the CC attribution block to be injected into API-key requests,
/// sending a misleading telemetry fingerprint.
#[test]
fn compute_attribution_returns_none_for_environment_credential() {
    use std::sync::Arc;

    use koina::credential::{Credential, CredentialProvider, CredentialSource};
    use koina::secret::SecretString;

    struct EnvKeyProvider;

    impl CredentialProvider for EnvKeyProvider {
        fn get_credential(&self) -> Option<Credential> {
            Some(Credential {
                secret: SecretString::from("sk-env-test-key"),
                source: CredentialSource::Environment,
            })
        }

        #[expect(
            clippy::unnecessary_literal_bound,
            reason = "trait requires &str return"
        )]
        fn name(&self) -> &str {
            "env-key"
        }
    }

    // Construct with first-party URL + cc_mimicry enabled (the pre-fix bug path).
    // with_credential_provider will set cc_profile = Some(...) because
    // is_first_party=true and cc_mimicry defaults to true.
    let config = ProviderConfig {
        base_url: Some("https://api.anthropic.com".to_owned()),
        cc_mimicry: Some(true),
        retry_policy: crate::RetryPolicy {
            max_retries: 0,
            ..crate::RetryPolicy::default()
        },
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::with_credential_provider(Arc::new(EnvKeyProvider), &config)
        .expect("valid config");

    // cc_profile must be Some (construction-time logic is unchanged).
    assert!(
        provider.cc_profile.is_some(),
        "cc_profile should be set for first-party + cc_mimicry=true"
    );

    // The regression: compute_attribution must return None despite cc_profile
    // being Some, because the runtime credential source is Environment.
    let result = provider.compute_attribution(&test_request());
    assert!(
        result.is_none(),
        "compute_attribution must return None for CredentialSource::Environment, got: {result:?}"
    );
}

/// #5886 complement: `compute_attribution` returns `Some` for OAuth credentials.
///
/// Ensures the fix does not regress the happy path — OAuth requests still
/// get the CC attribution block.
#[test]
fn compute_attribution_returns_some_for_oauth_credential() {
    use std::sync::Arc;

    use koina::credential::{Credential, CredentialProvider, CredentialSource};
    use koina::secret::SecretString;

    struct OAuthProvider;

    impl CredentialProvider for OAuthProvider {
        fn get_credential(&self) -> Option<Credential> {
            Some(Credential {
                secret: SecretString::from("oauth-access-token"),
                source: CredentialSource::OAuth,
            })
        }

        #[expect(
            clippy::unnecessary_literal_bound,
            reason = "trait requires &str return"
        )]
        fn name(&self) -> &str {
            "oauth"
        }
    }

    let config = ProviderConfig {
        base_url: Some("https://api.anthropic.com".to_owned()),
        cc_mimicry: Some(true),
        retry_policy: crate::RetryPolicy {
            max_retries: 0,
            ..crate::RetryPolicy::default()
        },
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::with_credential_provider(Arc::new(OAuthProvider), &config)
        .expect("valid config");

    assert!(
        provider.cc_profile.is_some(),
        "cc_profile should be set for first-party OAuth + cc_mimicry=true"
    );

    let result = provider.compute_attribution(&test_request());
    assert!(
        result.is_some(),
        "compute_attribution must return Some for CredentialSource::OAuth"
    );
}
